//! A user-chosen color-emoji font for the emoji picker's tile images.
//!
//! The picker normally draws each tile from a bundled Noto Color Emoji PNG
//! (see `Assets`). When the user picks their own font file from the tray,
//! tiles are drawn from that font's embedded bitmaps instead — the PNGs that
//! bitmap emoji fonts (`sbix`, `CBDT`) store per glyph — so Pasta never ships
//! the artwork itself. Any emoji the font can't supply falls back to the
//! bundled image. Text rendering elsewhere is unaffected.

use std::borrow::Cow;
use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use memmap2::Mmap;
use rustybuzz::{BufferFlags, Face, UnicodeBuffer};

/// Requested strike size. Tiles are 40px, so this covers 2x displays; fonts
/// return their closest strike.
const PIXELS_PER_EM: u16 = 96;
/// Rendered once on open to check the font actually carries bitmap emoji.
const PROBE_GLYPH: &str = "😀";
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub(crate) struct EmojiFont {
    path: PathBuf,
    // Memory-mapped rather than read: emoji fonts run to tens of megabytes
    // and the picker only ever touches a few dozen glyphs per open.
    data: Mmap,
}

#[derive(Debug)]
pub(crate) enum EmojiFontError {
    Read(std::io::Error),
    NotAFont,
    NoBitmapEmoji,
}

impl fmt::Display for EmojiFontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(err) => write!(f, "Couldn't read the file: {err}"),
            Self::NotAFont => write!(f, "The file isn't a font Pasta can read."),
            Self::NoBitmapEmoji => write!(
                f,
                "The font has no color bitmap emoji. Pasta supports sbix and CBDT emoji fonts, not vector (COLR or SVG) ones."
            ),
        }
    }
}

impl EmojiFont {
    pub(crate) fn open(path: &Path) -> Result<Self, EmojiFontError> {
        let file = File::open(path).map_err(EmojiFontError::Read)?;
        // SAFETY: the mapping is read-only and the font is only parsed, never
        // trusted for memory layout. A file truncated by another process while
        // mapped would fault on access; that's the accepted trade-off for not
        // holding a whole emoji font in memory.
        let data = unsafe { Mmap::map(&file) }.map_err(EmojiFontError::Read)?;
        let font = Self {
            path: path.to_owned(),
            data,
        };
        if Face::from_slice(&font.data, 0).is_none() {
            return Err(EmojiFontError::NotAFont);
        }
        if font.png(PROBE_GLYPH).is_none() {
            return Err(EmojiFontError::NoBitmapEmoji);
        }
        Ok(font)
    }

    pub(crate) fn display_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// The font's PNG for `glyph`, or `None` when the font has no single glyph
    /// for the whole sequence or stores it in a format other than PNG.
    /// Shaping (rather than a plain cmap lookup) is what resolves multi-
    /// codepoint emoji — ZWJ families, skin tones, flags, keycaps — to the
    /// font's ligature glyph.
    pub(crate) fn png(&self, glyph: &str) -> Option<Vec<u8>> {
        let face = Face::from_slice(&self.data, 0)?;
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(glyph);
        // Drops the leftover variation selectors and joiners instead of
        // keeping them as invisible glyphs, so a fully ligated sequence
        // shapes to exactly one glyph.
        buffer.set_flags(BufferFlags::REMOVE_DEFAULT_IGNORABLES);
        let shaped = rustybuzz::shape(&face, &[], buffer);
        let [info] = shaped.glyph_infos() else {
            return None;
        };
        let glyph_id = u16::try_from(info.glyph_id).ok().filter(|&id| id != 0)?;
        let image =
            face.glyph_raster_image(rustybuzz::ttf_parser::GlyphId(glyph_id), PIXELS_PER_EM)?;
        (image.format == rustybuzz::ttf_parser::RasterImageFormat::PNG
            && image.data.starts_with(PNG_SIGNATURE))
        .then(|| image.data.to_vec())
    }
}

static ACTIVE_FONT: RwLock<Option<Arc<EmojiFont>>> = RwLock::new(None);
/// Bumped on every font change and baked into tile image paths, since gpui
/// caches loaded images by path and would otherwise keep the old artwork.
static FONT_GENERATION: AtomicU64 = AtomicU64::new(0);

pub(crate) fn set_active_emoji_font(font: Option<EmojiFont>) {
    *ACTIVE_FONT
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = font.map(Arc::new);
    FONT_GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn active_emoji_font() -> Option<Arc<EmojiFont>> {
    ACTIVE_FONT
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// The asset path for a tile's image: the bundled PNG, or — while a custom
/// font is active — a generation-stamped path `load_emoji_image` serves from
/// that font. Bundled PNGs are keyed by codepoints in lowercase hex joined
/// with `-` (see `assets/emoji/ATTRIBUTION.md`).
pub(crate) fn emoji_image_path(glyph: &str) -> String {
    let key = glyph
        .chars()
        .map(|c| format!("{:x}", c as u32))
        .collect::<Vec<_>>()
        .join("-");
    if active_emoji_font().is_some() {
        format!(
            "emoji-font/{}/{key}.png",
            FONT_GENERATION.load(Ordering::Relaxed)
        )
    } else {
        format!("emoji/{key}.png")
    }
}

/// Serves an `emoji-font/<generation>/<key>.png` path (the part after
/// `emoji-font/`) from the active font, falling back to the bundled PNG
/// for anything the font can't supply.
pub(crate) fn load_emoji_image(
    path: &str,
    bundled: impl FnOnce(&str) -> Option<Cow<'static, [u8]>>,
) -> Option<Cow<'static, [u8]>> {
    let (_generation, file_name) = path.split_once('/')?;
    let from_font = file_name
        .strip_suffix(".png")
        .and_then(glyph_from_key)
        .zip(active_emoji_font())
        .and_then(|(glyph, font)| font.png(&glyph));
    match from_font {
        Some(png) => Some(Cow::Owned(png)),
        None => bundled(file_name),
    }
}

fn glyph_from_key(key: &str) -> Option<String> {
    key.split('-')
        .map(|hex| u32::from_str_radix(hex, 16).ok().and_then(char::from_u32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM_NOTO_COLOR_EMOJI: &str = "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf";

    fn repo_file(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    /// The only bitmap emoji font a dev machine reliably has; tests that need
    /// one skip (loudly) when the distro doesn't ship it at this path.
    fn system_noto() -> Option<EmojiFont> {
        let path = Path::new(SYSTEM_NOTO_COLOR_EMOJI);
        if !path.exists() {
            eprintln!("skipping: {SYSTEM_NOTO_COLOR_EMOJI} not installed");
            return None;
        }
        Some(EmojiFont::open(path).expect("system Noto Color Emoji opens"))
    }

    #[test]
    fn open_rejects_a_file_that_is_not_a_font() {
        let err = EmojiFont::open(&repo_file("Cargo.toml")).err();
        assert!(matches!(err, Some(EmojiFontError::NotAFont)), "{err:?}");
    }

    #[test]
    fn open_rejects_an_outline_font_without_bitmap_emoji() {
        let err = EmojiFont::open(&repo_file("assets/fonts/Geist-Regular.ttf")).err();
        assert!(
            matches!(err, Some(EmojiFontError::NoBitmapEmoji)),
            "{err:?}"
        );
    }

    #[test]
    fn open_reports_a_missing_file() {
        let err = EmojiFont::open(&repo_file("does-not-exist.ttf")).err();
        assert!(matches!(err, Some(EmojiFontError::Read(_))), "{err:?}");
    }

    #[test]
    fn bitmap_font_yields_pngs_for_single_and_sequence_emoji() {
        let Some(font) = system_noto() else {
            return;
        };
        for glyph in [
            "😀",
            "❤\u{fe0f}",
            "👍\u{1f3fd}",
            "🇺🇸",
            "1\u{fe0f}\u{20e3}",
            "👨\u{200d}👩\u{200d}👧",
        ] {
            let png = font.png(glyph);
            assert!(
                png.is_some_and(|png| png.starts_with(PNG_SIGNATURE)),
                "no PNG for {glyph:?}"
            );
        }
    }

    #[test]
    fn bitmap_font_has_no_png_for_plain_text() {
        let Some(font) = system_noto() else {
            return;
        };
        assert!(font.png("ab").is_none());
    }

    /// The only test that touches the process-wide active font, so it can't
    /// race the others.
    #[test]
    fn image_paths_serve_the_active_font_and_fall_back_to_bundled() {
        let Some(font) = system_noto() else {
            return;
        };
        let expected = font.png("😀").expect("font has 😀");
        let bundled = |name: &str| Some(Cow::Owned(format!("bundled:{name}").into_bytes()));

        assert_eq!(emoji_image_path("😀"), "emoji/1f600.png");
        set_active_emoji_font(Some(font));
        let path = emoji_image_path("😀");
        let rest = path.strip_prefix("emoji-font/").expect("font-backed path");
        assert_eq!(
            load_emoji_image(rest, bundled).as_deref(),
            Some(expected.as_slice())
        );
        // A key the font has no glyph for comes from the bundled set.
        let missing = load_emoji_image("0/61-62.png", bundled);
        assert_eq!(missing.as_deref(), Some(b"bundled:61-62.png".as_slice()));

        set_active_emoji_font(None);
        assert_eq!(emoji_image_path("😀"), "emoji/1f600.png");
    }

    #[test]
    fn glyph_key_round_trips_through_the_image_path_key() {
        assert_eq!(glyph_from_key("1f1fa-1f1f8").as_deref(), Some("🇺🇸"));
        assert_eq!(glyph_from_key("2764-fe0f").as_deref(), Some("❤\u{fe0f}"));
        assert_eq!(glyph_from_key("zz"), None);
    }
}
