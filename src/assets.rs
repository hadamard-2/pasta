use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

/// Serves the SVG icons bundled under `assets/icons/` to gpui's `svg()`
/// element, which resolves `.path(...)` lookups through this at paint time.
pub(crate) struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
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
