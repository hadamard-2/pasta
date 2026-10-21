use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::neural_embed::{self, NeuralEmbedder};
use crate::storage::{cosine_similarity, semantic_tokenize};

/// Unicode CLDR English annotations (SPDX Unicode-3.0). For emoji they only
/// enrich each entry's keyword list beyond the crate's built-in shortcodes —
/// e.g. covering mood synonyms like "cheerful" or "awesome" that shortcodes
/// miss. The same file also annotates a curated set of non-emoji characters
/// (dashes, punctuation, arrows, math, currency), which is the whole corpus
/// for symbol search.
const CLDR_ANNOTATIONS_EN: &str = include_str!("../assets/emoji/cldr-annotations-en.xml");

pub(crate) struct EmojiEntry {
    pub(crate) glyph: String,
    pub(crate) name: String,
    keywords: Vec<String>,
    search_terms: Vec<String>,
}

static EMOJI_ENTRIES: LazyLock<Vec<EmojiEntry>> = LazyLock::new(build_emoji_entries);
static SYMBOL_ENTRIES: LazyLock<Vec<EmojiEntry>> = LazyLock::new(build_symbol_entries);
static CORPUS_EMBEDDINGS: Mutex<Option<Vec<Vec<f32>>>> = Mutex::new(None);

fn build_emoji_entries() -> Vec<EmojiEntry> {
    let cldr_keywords = parse_cldr_annotations(CLDR_ANNOTATIONS_EN);

    emojis::iter()
        .map(|emoji| {
            let name = emoji.name().to_string();
            let mut keywords: Vec<String> = emoji.shortcodes().map(str::to_owned).collect();
            // CLDR keys omit the U+FE0F variation selector that the crate
            // spells text-default emoji with (`❤` vs `❤️`).
            let extra = cldr_keywords
                .get(emoji.as_str())
                .or_else(|| cldr_keywords.get(emoji.as_str().replace('\u{fe0f}', "").as_str()));
            if let Some(extra) = extra {
                for keyword in extra {
                    if !keywords.contains(keyword) {
                        keywords.push(keyword.clone());
                    }
                }
            }

            EmojiEntry::new(emoji.as_str().to_owned(), name, keywords)
        })
        .collect()
}

/// The non-emoji characters CLDR annotates, in file order (which groups them
/// by kind — punctuation, then arrows, math, currency and so on).
fn build_symbol_entries() -> Vec<EmojiEntry> {
    parse_cldr_symbol_annotations(CLDR_ANNOTATIONS_EN)
        .into_iter()
        .filter(|(glyph, _, _)| is_text_symbol(glyph))
        .map(|(glyph, name, keywords)| EmojiEntry::new(glyph, name, keywords))
        .collect()
}

/// Whether a CLDR-annotated character belongs in symbol search. Printable
/// ASCII is skipped since it's already on the keyboard. Emoji are skipped
/// since emoji search covers them, with one exception: characters like `©`,
/// `™` or `↔` that render as plain text unless followed by U+FE0F — the
/// `emojis` crate lists them under that FE0F form, and the bare character is
/// the one people want when writing. Anything in the pictograph planes is
/// treated as emoji even when the bundled `emojis` crate predates it, along
/// with skin-tone and hair components.
fn is_text_symbol(glyph: &str) -> bool {
    if glyph.is_ascii() {
        return false;
    }
    if glyph
        .chars()
        .any(|c| matches!(c as u32, 0x1F000..=0x1FAFF | 0xFE0F | 0x200D))
    {
        return false;
    }
    emojis::get(glyph).is_none_or(|emoji| emoji.as_str() != glyph)
}

impl EmojiEntry {
    fn new(glyph: String, name: String, keywords: Vec<String>) -> Self {
        let mut search_terms = semantic_tokenize(&name);
        for keyword in &keywords {
            search_terms.extend(semantic_tokenize(keyword));
        }
        search_terms.sort();
        search_terms.dedup();

        Self {
            glyph,
            name,
            keywords,
            search_terms,
        }
    }
}

/// Parses `<annotation cp="X">a | b | c</annotation>` lines, skipping the
/// paired `type="tts"` line (that's just the display name, already covered
/// by the `emojis` crate). The format is regular enough that a full XML
/// parser would be overkill for this one-time startup pass.
fn parse_cldr_annotations(xml: &str) -> HashMap<&str, Vec<String>> {
    let mut map = HashMap::new();

    for line in xml.lines() {
        let Some(annotation) = parse_annotation_line(line) else {
            continue;
        };
        if annotation.is_tts {
            continue;
        }
        let keywords = split_keywords(annotation.body);
        if !keywords.is_empty() {
            map.insert(annotation.cp, keywords);
        }
    }

    map
}

/// Pairs each annotated character with its `type="tts"` display name and
/// keyword list, as `(glyph, name, keywords)` in file order. Unlike
/// `parse_cldr_annotations` the code point is unescaped, since symbols
/// include `&`, `<` and `>`. Characters missing either line are dropped.
fn parse_cldr_symbol_annotations(xml: &str) -> Vec<(String, String, Vec<String>)> {
    let mut names: HashMap<&str, String> = HashMap::new();
    let mut keyword_lines: Vec<(&str, Vec<String>)> = Vec::new();

    for line in xml.lines() {
        let Some(annotation) = parse_annotation_line(line) else {
            continue;
        };
        if annotation.is_tts {
            names.insert(annotation.cp, unescape_xml_entities(annotation.body.trim()));
        } else {
            let keywords = split_keywords(annotation.body);
            if !keywords.is_empty() {
                keyword_lines.push((annotation.cp, keywords));
            }
        }
    }

    keyword_lines
        .into_iter()
        .filter_map(|(cp, keywords)| {
            let name = names.remove(cp)?;
            Some((unescape_xml_entities(cp), name, keywords))
        })
        .collect()
}

struct AnnotationLine<'a> {
    cp: &'a str,
    is_tts: bool,
    body: &'a str,
}

fn parse_annotation_line(line: &str) -> Option<AnnotationLine<'_>> {
    let line = line.trim();
    let rest = line.strip_prefix("<annotation cp=\"")?;
    let cp_end = rest.find('"')?;
    let tag_close = rest.find('>')?;
    let body_start = tag_close + 1;
    let body_len = rest[body_start..].find("</annotation>")?;
    Some(AnnotationLine {
        cp: &rest[..cp_end],
        is_tts: rest[..tag_close].contains("type=\"tts\""),
        body: &rest[body_start..body_start + body_len],
    })
}

fn split_keywords(body: &str) -> Vec<String> {
    body.split('|')
        .map(|keyword| unescape_xml_entities(keyword.trim()))
        .filter(|keyword| !keyword.is_empty())
        .collect()
}

fn unescape_xml_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Which corpus the launcher's glyph-picker grid is showing. Both share the
/// same grid, keyboard handling and copy-on-Enter behavior; only the corpus
/// and its labels differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GlyphPicker {
    Emoji,
    Symbol,
}

impl GlyphPicker {
    /// The picker whose affordance row the query is reaching for: a non-empty
    /// prefix (trimmed, case-insensitive) of "emoji" or "symbol".
    pub(crate) fn for_affordance_query(query: &str) -> Option<Self> {
        let normalized = query.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            None
        } else if "emoji".starts_with(normalized.as_str()) {
            Some(Self::Emoji)
        } else if "symbol".starts_with(normalized.as_str()) {
            Some(Self::Symbol)
        } else {
            None
        }
    }

    /// Glyph and display name for a `search` result index, for rendering.
    pub(crate) fn entry_at(self, index: usize) -> Option<(&'static str, &'static str)> {
        self.entries()
            .get(index)
            .map(|entry| (entry.glyph.as_str(), entry.name.as_str()))
    }

    /// Result indices for `query`, best first. An empty query lists the whole
    /// corpus in its natural grouped order so every glyph is reachable by
    /// scrolling; `limit` only caps ranked results for a non-empty query.
    pub(crate) fn search(
        self,
        query: &str,
        embedder: Option<&NeuralEmbedder>,
        limit: usize,
    ) -> Vec<usize> {
        match self {
            Self::Emoji => search_emojis(query, embedder, limit),
            Self::Symbol => search_symbols(query, limit),
        }
    }

    fn entries(self) -> &'static [EmojiEntry] {
        match self {
            Self::Emoji => &EMOJI_ENTRIES,
            Self::Symbol => &SYMBOL_ENTRIES,
        }
    }
}

/// Ranks the symbol corpus lexically. Unlike emoji there is no neural tier:
/// symbol names are short and literal ("en dash", "not equal"), so the CLDR
/// keywords already carry the synonyms that matter.
fn search_symbols(query: &str, limit: usize) -> Vec<usize> {
    let query = query.trim();
    if query.is_empty() {
        return (0..SYMBOL_ENTRIES.len()).collect();
    }

    let query_lower = query.to_ascii_lowercase();
    let query_terms = query_terms(query);
    // Ties go to the lower code point, which favors the everyday Latin-1
    // characters over look-alikes from other scripts (`·` over `・`).
    rank(
        lexical_only_scores(&SYMBOL_ENTRIES, &query_lower, &query_terms),
        limit,
        |index| {
            SYMBOL_ENTRIES[index]
                .glyph
                .chars()
                .next()
                .map_or(0, u32::from)
        },
    )
}

/// Ranks the emoji corpus against `query`. Lexical matches (exact keyword,
/// then substring) always outrank semantic-only ones, mirroring the same
/// hybrid model clipboard search uses in `storage::combined_search_score` —
/// `embedder` is only used when pasta-brain is enabled, so this degrades to
/// lexical-only search when it's off or not yet loaded.
fn search_emojis(query: &str, embedder: Option<&NeuralEmbedder>, limit: usize) -> Vec<usize> {
    let query = query.trim();
    if query.is_empty() {
        return (0..EMOJI_ENTRIES.len()).collect();
    }

    let query_lower = query.to_ascii_lowercase();
    let query_terms = query_terms(query);

    let scored: Vec<(f32, usize)> = embedder
        .and_then(|embedder| {
            let query_embedding = embedder.embed(query, &query_terms);
            with_corpus_embeddings(|corpus_embeddings: &[Vec<f32>]| {
                EMOJI_ENTRIES
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| {
                        let lexical = lexical_emoji_score(entry, &query_lower, &query_terms);
                        let neural = cosine_similarity(&query_embedding, &corpus_embeddings[index]);
                        let score = if lexical > 0.0 {
                            2.0 + lexical + neural * 0.5
                        } else {
                            neural * 0.65
                        };
                        (score > 0.05).then_some((score, index))
                    })
                    .collect::<Vec<(f32, usize)>>()
            })
        })
        .unwrap_or_else(|| lexical_only_scores(&EMOJI_ENTRIES, &query_lower, &query_terms));

    rank(scored, limit, |index| index as u32)
}

/// `semantic_tokenize` emits a plain word twice (whole, then as its only
/// split part), which would double-count every term in the lexical score.
fn query_terms(query: &str) -> Vec<String> {
    let mut terms = semantic_tokenize(query);
    terms.sort();
    terms.dedup();
    terms
}

/// Highest score first; equal scores fall back to ascending `tie_break`.
fn rank(
    mut scored: Vec<(f32, usize)>,
    limit: usize,
    tie_break: impl Fn(usize) -> u32,
) -> Vec<usize> {
    scored.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| tie_break(left.1).cmp(&tie_break(right.1)))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, index)| index)
        .collect()
}

fn lexical_only_scores(
    entries: &[EmojiEntry],
    query_lower: &str,
    query_terms: &[String],
) -> Vec<(f32, usize)> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let lexical = lexical_emoji_score(entry, query_lower, query_terms);
            (lexical > 0.0).then_some((lexical, index))
        })
        .collect()
}

fn lexical_emoji_score(entry: &EmojiEntry, query_lower: &str, query_terms: &[String]) -> f32 {
    // An exact name match, then an exact keyword match, outranks any partial
    // one: the partial sum below tops out at one point per query term.
    if entry.name == *query_lower {
        return query_terms.len() as f32 + 2.0;
    }
    if entry.keywords.iter().any(|keyword| keyword == query_lower) {
        return query_terms.len() as f32 + 1.0;
    }

    let mut score = 0.0;
    for term in query_terms {
        if entry.search_terms.iter().any(|candidate| candidate == term) {
            score += 1.0;
        } else if entry
            .search_terms
            .iter()
            .any(|candidate| candidate.contains(term.as_str()))
        {
            score += 0.5;
        }
    }
    score
}

/// Reads the cached corpus embeddings if `prewarm_corpus_embeddings` has
/// already populated them, without blocking to compute them — a query typed
/// before the background prewarm finishes just falls back to lexical-only
/// scoring for that one keystroke instead of stalling the UI thread.
fn with_corpus_embeddings<R>(f: impl FnOnce(&[Vec<f32>]) -> R) -> Option<R> {
    let guard = CORPUS_EMBEDDINGS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.as_ref().map(|embeddings| f(embeddings))
}

/// Populates the corpus embedding cache — from an on-disk cache when one
/// matches the current model/corpus, otherwise via a batched inference call
/// (see `NeuralEmbedder::embed_batch`) whose result is then written to disk
/// for next launch. Call this once, off the UI thread, right after the
/// neural embedder finishes initializing (see `spawn_neural_init`) — without
/// it, the first non-empty emoji search of a session would otherwise pay for
/// ~1,900 sequential, synchronous embedding calls on the UI thread (multiple
/// seconds).
pub(crate) fn prewarm_corpus_embeddings(embedder: &NeuralEmbedder) {
    if let Some(cached) = load_cached_corpus_embeddings() {
        *CORPUS_EMBEDDINGS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(cached);
        return;
    }

    let Some(embeddings) = embedder.embed_batch(
        EMOJI_ENTRIES
            .iter()
            .map(|entry| (entry.name.as_str(), entry.keywords.as_slice())),
    ) else {
        // Leave the cache unpopulated so `search_emojis` degrades to
        // lexical-only for this session, and leave nothing on disk so a
        // transient failure (e.g. an ORT hiccup) gets retried next launch
        // instead of being trusted forever as a "valid" empty cache.
        return;
    };
    save_corpus_embeddings_to_cache(&embeddings);
    *CORPUS_EMBEDDINGS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(embeddings);
}

#[derive(Serialize, Deserialize)]
struct CorpusEmbeddingCacheMeta {
    model_version: String,
    corpus_fingerprint: String,
}

fn corpus_embedding_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("pasta-launcher")
        .join("emoji-embeddings")
}

/// Fingerprints the corpus content that actually feeds the embedder (name +
/// keywords per entry) so an on-disk cache can be invalidated if a future
/// `emojis`/CLDR-annotations update changes what gets embedded.
fn corpus_fingerprint() -> String {
    let mut hasher = Sha256::new();
    for entry in EMOJI_ENTRIES.iter() {
        hasher.update(entry.name.as_bytes());
        hasher.update(b"\0");
        for keyword in &entry.keywords {
            hasher.update(keyword.as_bytes());
            hasher.update(b"\0");
        }
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

/// Loads corpus embeddings from disk if a cache exists and its metadata
/// matches the current model version and corpus fingerprint; `None` on any
/// mismatch, missing file, or malformed content, so the caller just falls
/// back to recomputing — a stale or corrupt cache should never be trusted.
fn load_cached_corpus_embeddings() -> Option<Vec<Vec<f32>>> {
    let dir = corpus_embedding_cache_dir();
    let meta_bytes = fs::read(dir.join("meta.json")).ok()?;
    let meta: CorpusEmbeddingCacheMeta = serde_json::from_slice(&meta_bytes).ok()?;
    if meta.model_version != neural_embed::MODEL_VERSION
        || meta.corpus_fingerprint != corpus_fingerprint()
    {
        return None;
    }

    let blob = fs::read(dir.join("embeddings.bin")).ok()?;
    let bytes_per_row = neural_embed::NEURAL_VECTOR_DIM * 4;
    if bytes_per_row == 0 || blob.len() % bytes_per_row != 0 {
        return None;
    }
    if blob.len() / bytes_per_row != EMOJI_ENTRIES.len() {
        return None;
    }

    Some(
        blob.chunks_exact(bytes_per_row)
            .map(|row| {
                row.chunks_exact(4)
                    .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
                    .collect()
            })
            .collect(),
    )
}

/// Writes the embeddings blob first and the metadata sidecar last — the
/// sidecar is the "this cache is valid" marker, so if the process dies
/// mid-save, either the sidecar still points at the old (untouched) blob or
/// it isn't written at all; both cases fail validation cleanly on next load
/// instead of trusting a half-written blob under a freshly-written sidecar.
fn save_corpus_embeddings_to_cache(embeddings: &[Vec<f32>]) {
    let dir = corpus_embedding_cache_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }

    let mut blob = Vec::with_capacity(embeddings.len() * neural_embed::NEURAL_VECTOR_DIM * 4);
    for vector in embeddings {
        for value in vector {
            blob.extend_from_slice(&value.to_le_bytes());
        }
    }
    if fs::write(dir.join("embeddings.bin"), blob).is_err() {
        return;
    }

    let meta = CorpusEmbeddingCacheMeta {
        model_version: neural_embed::MODEL_VERSION.to_owned(),
        corpus_fingerprint: corpus_fingerprint(),
    };
    let Ok(meta_json) = serde_json::to_vec(&meta) else {
        return;
    };
    let _ = fs::write(dir.join("meta.json"), meta_json);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn top_symbol(query: &str) -> Option<&'static str> {
        GlyphPicker::Symbol
            .search(query, None, 1)
            .first()
            .and_then(|&index| GlyphPicker::Symbol.entry_at(index))
            .map(|(glyph, _)| glyph)
    }

    #[test]
    fn empty_query_lists_the_whole_corpus_regardless_of_limit() {
        for picker in [GlyphPicker::Emoji, GlyphPicker::Symbol] {
            let results = picker.search("  ", None, 1);
            assert_eq!(results.len(), picker.entries().len());
            assert!(results.into_iter().eq(0..picker.entries().len()));
        }
    }

    #[test]
    fn symbol_search_finds_characters_by_name_and_keyword() {
        assert_eq!(top_symbol("en dash"), Some("–"));
        assert_eq!(top_symbol("em dash"), Some("—"));
        assert_eq!(top_symbol("interpunct"), Some("·"));
        assert_eq!(top_symbol("not equal"), Some("≠"));
    }

    #[test]
    fn symbol_search_includes_text_style_emoji_characters() {
        assert_eq!(top_symbol("copyright"), Some("©"));
        assert_eq!(top_symbol("trade mark"), Some("™"));
    }

    #[test]
    fn symbol_corpus_skips_keyboard_ascii_and_emoji() {
        let glyphs: Vec<&str> = SYMBOL_ENTRIES.iter().map(|e| e.glyph.as_str()).collect();
        assert!(glyphs.iter().all(|glyph| !glyph.is_ascii()));
        for emoji in ["⭐", "🏻", "🦰", "🪌"] {
            assert!(!glyphs.contains(&emoji), "{emoji} is emoji, not a symbol");
        }
        assert!(SYMBOL_ENTRIES.iter().all(|entry| !entry.name.is_empty()));
    }

    #[test]
    fn emoji_keywords_include_cldr_annotations_for_variation_selector_emoji() {
        let heart = EMOJI_ENTRIES
            .iter()
            .find(|entry| entry.glyph == "❤\u{fe0f}")
            .expect("red heart is in the emoji corpus");
        assert!(heart.keywords.iter().any(|keyword| keyword == "love"));
    }

    #[test]
    fn affordance_query_picks_the_matching_picker() {
        assert_eq!(
            GlyphPicker::for_affordance_query("e"),
            Some(GlyphPicker::Emoji)
        );
        assert_eq!(
            GlyphPicker::for_affordance_query(" EMOJI "),
            Some(GlyphPicker::Emoji)
        );
        assert_eq!(
            GlyphPicker::for_affordance_query("sym"),
            Some(GlyphPicker::Symbol)
        );
        assert_eq!(GlyphPicker::for_affordance_query("symbols"), None);
        assert_eq!(GlyphPicker::for_affordance_query(""), None);
    }
}
