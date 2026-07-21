use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::neural_embed::{self, NeuralEmbedder};
use crate::storage::{cosine_similarity, semantic_tokenize};

/// Unicode CLDR English annotations (SPDX Unicode-3.0), used only to enrich
/// each emoji's keyword list beyond the crate's built-in shortcodes — e.g.
/// covering mood synonyms like "cheerful" or "awesome" that shortcodes miss.
const CLDR_ANNOTATIONS_EN: &str = include_str!("../assets/emoji/cldr-annotations-en.xml");

pub(crate) struct EmojiEntry {
    pub(crate) glyph: &'static str,
    pub(crate) name: String,
    keywords: Vec<String>,
    search_terms: Vec<String>,
}

static EMOJI_ENTRIES: LazyLock<Vec<EmojiEntry>> = LazyLock::new(build_emoji_entries);
static CORPUS_EMBEDDINGS: Mutex<Option<Vec<Vec<f32>>>> = Mutex::new(None);

fn build_emoji_entries() -> Vec<EmojiEntry> {
    let cldr_keywords = parse_cldr_annotations(CLDR_ANNOTATIONS_EN);

    emojis::iter()
        .map(|emoji| {
            let name = emoji.name().to_string();
            let mut keywords: Vec<String> = emoji.shortcodes().map(str::to_owned).collect();
            if let Some(extra) = cldr_keywords.get(emoji.as_str()) {
                for keyword in extra {
                    if !keywords.contains(keyword) {
                        keywords.push(keyword.clone());
                    }
                }
            }

            let mut search_terms = semantic_tokenize(&name);
            for keyword in &keywords {
                search_terms.extend(semantic_tokenize(keyword));
            }
            search_terms.sort();
            search_terms.dedup();

            EmojiEntry {
                glyph: emoji.as_str(),
                name,
                keywords,
                search_terms,
            }
        })
        .collect()
}

/// Parses `<annotation cp="X">a | b | c</annotation>` lines, skipping the
/// paired `type="tts"` line (that's just the display name, already covered
/// by the `emojis` crate). The format is regular enough that a full XML
/// parser would be overkill for this one-time startup pass.
fn parse_cldr_annotations(xml: &str) -> HashMap<&str, Vec<String>> {
    let mut map = HashMap::new();

    for line in xml.lines() {
        let line = line.trim();
        if line.contains("type=\"tts\"") {
            continue;
        }
        let Some(rest) = line.strip_prefix("<annotation cp=\"") else {
            continue;
        };
        let Some(cp_end) = rest.find('"') else {
            continue;
        };
        let cp = &rest[..cp_end];
        let Some(tag_close) = rest.find('>') else {
            continue;
        };
        let body_start = tag_close + 1;
        let Some(body_len) = rest[body_start..].find("</annotation>") else {
            continue;
        };
        let body = &rest[body_start..body_start + body_len];

        let keywords: Vec<String> = body
            .split('|')
            .map(|keyword| unescape_xml_entities(keyword.trim()))
            .filter(|keyword| !keyword.is_empty())
            .collect();
        if !keywords.is_empty() {
            map.insert(cp, keywords);
        }
    }

    map
}

fn unescape_xml_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// True when the (trimmed, lowercased) query is a non-empty prefix of
/// "emoji" — the trigger for surfacing the emoji-search affordance row.
pub(crate) fn should_show_emoji_affordance(query: &str) -> bool {
    let normalized = query.trim().to_ascii_lowercase();
    !normalized.is_empty() && "emoji".starts_with(normalized.as_str())
}

/// Glyph and display name for a `search_emojis` result index, for rendering.
pub(crate) fn entry_at(index: usize) -> Option<(&'static str, &'static str)> {
    EMOJI_ENTRIES
        .get(index)
        .map(|entry| (entry.glyph, entry.name.as_str()))
}

/// Ranks the emoji corpus against `query`. Lexical matches (exact keyword,
/// then substring) always outrank semantic-only ones, mirroring the same
/// hybrid model clipboard search uses in `storage::combined_search_score` —
/// `embedder` is only used when pasta-brain is enabled, so this degrades to
/// lexical-only search when it's off or not yet loaded.
pub(crate) fn search_emojis(
    query: &str,
    embedder: Option<&NeuralEmbedder>,
    limit: usize,
) -> Vec<usize> {
    let query = query.trim();
    if query.is_empty() {
        return (0..EMOJI_ENTRIES.len().min(limit)).collect();
    }

    let query_lower = query.to_ascii_lowercase();
    let query_terms = semantic_tokenize(query);

    let mut scored: Vec<(f32, usize)> = embedder
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
        .unwrap_or_else(|| lexical_only_scores(&query_lower, &query_terms));

    scored.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, index)| index)
        .collect()
}

fn lexical_only_scores(query_lower: &str, query_terms: &[String]) -> Vec<(f32, usize)> {
    EMOJI_ENTRIES
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let lexical = lexical_emoji_score(entry, query_lower, query_terms);
            (lexical > 0.0).then_some((lexical, index))
        })
        .collect()
}

fn lexical_emoji_score(entry: &EmojiEntry, query_lower: &str, query_terms: &[String]) -> f32 {
    if entry.name == *query_lower || entry.keywords.iter().any(|keyword| keyword == query_lower) {
        return 3.0;
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
