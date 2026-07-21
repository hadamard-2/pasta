use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

/// Serves bundled assets to gpui's `svg()`/`img()` elements, which resolve
/// `.path(...)` / embedded-resource lookups through this at paint time:
/// the SVG icons under `assets/icons/`, and (on Linux) the emoji PNGs under
/// `assets/emoji/png/` used by the emoji picker's image-based rendering.
pub(crate) struct Assets;

/// The emoji picker renders each glyph from a bundled Noto Color Emoji PNG
/// (keyed by the glyph's codepoints joined with `-`), sidestepping the Linux
/// cosmic-text/GPUI color-emoji font-fallback limitations. Linux-only —
/// macOS renders emoji natively and embeds none of this.
#[cfg(target_os = "linux")]
#[derive(rust_embed::RustEmbed)]
#[folder = "assets/emoji/png"]
struct EmojiPngs;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        #[cfg(target_os = "linux")]
        if let Some(name) = path.strip_prefix("emoji/") {
            return Ok(EmojiPngs::get(name).map(|file| file.data));
        }
        match path {
            "icons/search.svg" => Ok(Some(Cow::Borrowed(
                include_bytes!("../assets/icons/search.svg").as_slice(),
            ))),
            _ => Ok(None),
        }
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
