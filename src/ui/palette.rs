use super::LanguageTag;
use crate::*;

#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) dark: bool,
    pub(crate) window_bg: gpui::Rgba,
    pub(crate) window_border: gpui::Rgba,
    pub(crate) title_text: gpui::Rgba,
    pub(crate) query_placeholder: gpui::Rgba,
    pub(crate) query_active: gpui::Rgba,
    pub(crate) muted_text: gpui::Rgba,
    pub(crate) list_divider: gpui::Rgba,
    pub(crate) row_text: gpui::Rgba,
    pub(crate) row_meta_text: gpui::Rgba,
    pub(crate) row_hover_bg: gpui::Rgba,
    pub(crate) selected_bg: gpui::Rgba,
    pub(crate) selected_border: gpui::Rgba,
    pub(crate) action_bar_bg: gpui::Rgba,
    pub(crate) keycap_bg: gpui::Rgba,
    pub(crate) keycap_text: gpui::Rgba,
    /// Pasta's brand accent, used sparingly (the action bar status dot,
    /// the primary action keycap) — the rest of the UI stays on the
    /// neutral gray ladder by design.
    pub(crate) accent: gpui::Rgba,
}

pub(crate) fn palette_for(surface_alpha: f32) -> Palette {
    // A neutral, near-zero-chroma grayscale ladder. The window surface is opaque
    // by default (blur is unavailable on GNOME); the border plus shadow separate
    // it from the desktop. surface_alpha still scales the surfaces below 1.0.
    // Pasta is dark-only by design; there is no light-mode branch to pick between.
    let mut palette = Palette {
        dark: true,
        window_bg: rgba(0x17171Aff),
        window_border: rgba(0x2C2C30ff),
        title_text: rgba(0xEDEDEFff),
        query_placeholder: rgba(0x6E6E73ff),
        query_active: rgba(0xF5F5F7ff),
        muted_text: rgba(0x8A8A8Fff),
        list_divider: rgba(0x262629ff),
        row_text: rgba(0xEDEDEFff),
        row_meta_text: rgba(0x8A8A8Fff),
        row_hover_bg: rgba(0xFFFFFF0A),
        selected_bg: rgba(0x2A2A2Eff),
        selected_border: rgba(0x00000000),
        action_bar_bg: rgba(0x1C1C1Fff),
        keycap_bg: rgba(0x2A2A2Eff),
        keycap_text: rgba(0x8A8A8Fff),
        accent: rgba(0x00664Cff),
    };

    // Scale window surface elements by the fixed alpha factor.
    let alpha_scale = surface_alpha.clamp(0.45, 1.0);
    palette.window_bg = scale_alpha(palette.window_bg, alpha_scale);
    palette.window_border = scale_alpha(palette.window_border, alpha_scale);
    palette.list_divider = scale_alpha(palette.list_divider, alpha_scale);
    palette.row_hover_bg = scale_alpha(palette.row_hover_bg, alpha_scale);
    palette.selected_bg = scale_alpha(palette.selected_bg, alpha_scale);
    palette.selected_border = scale_alpha(palette.selected_border, alpha_scale);

    palette
}

pub(crate) fn scale_alpha(color: gpui::Rgba, scale: f32) -> gpui::Rgba {
    gpui::Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: (color.a * scale).clamp(0.0, 1.0),
    }
}

pub(crate) fn type_color(item_type: ClipboardItemType, dark: bool) -> gpui::Hsla {
    // Heavily desaturated type hues. Color lands only on the leading row icon
    // (and detail-pane chips), never on a saturated chip background.
    match item_type {
        ClipboardItemType::Text => {
            if dark {
                rgb(0x6E9ECF).into()
            } else {
                rgb(0x4E7BA6).into()
            }
        }
        ClipboardItemType::Code => {
            if dark {
                rgb(0x5FB99A).into()
            } else {
                rgb(0x3F8C6E).into()
            }
        }
        ClipboardItemType::Command => {
            if dark {
                rgb(0xCFA85F).into()
            } else {
                rgb(0x9A7636).into()
            }
        }
        ClipboardItemType::Password => {
            if dark {
                rgb(0xCF7FA3).into()
            } else {
                rgb(0xA85F81).into()
            }
        }
    }
}

/// A single-glyph leading indicator per clipboard type, colored via
/// [`type_color`]. Kept to plain ASCII so it renders in any bundled font.
pub(crate) fn type_icon_glyph(item_type: ClipboardItemType) -> &'static str {
    match item_type {
        ClipboardItemType::Text => "T",
        ClipboardItemType::Code => "#",
        ClipboardItemType::Command => "$",
        ClipboardItemType::Password => "*",
    }
}

pub(crate) fn tag_chip_color(label: &str, dark: bool) -> gpui::Hsla {
    if label.starts_with("OPEN ") {
        if dark {
            return rgb(0x4ade80).into();
        }
        return rgb(0x15803d).into();
    }
    if label.starts_with("P:") {
        if dark {
            return rgb(0x67e8f9).into();
        }
        return rgb(0x0e7490).into();
    }

    match label {
        "LOCKED" => {
            if dark {
                rgb(0xfb7185).into()
            } else {
                rgb(0xbe123c).into()
            }
        }
        "TEXT" => type_color(ClipboardItemType::Text, dark),
        "CODE" => type_color(ClipboardItemType::Code, dark),
        "CMD" => type_color(ClipboardItemType::Command, dark),
        "PASS" | "SECRET" => type_color(ClipboardItemType::Password, dark),
        "BASH" => language_color(LanguageTag::Bash, dark),
        "RUST" => language_color(LanguageTag::Rust, dark),
        "PY" => language_color(LanguageTag::Python, dark),
        "TS" => language_color(LanguageTag::TypeScript, dark),
        "JS" => language_color(LanguageTag::JavaScript, dark),
        "GO" => language_color(LanguageTag::Go, dark),
        "JAVA" => language_color(LanguageTag::Java, dark),
        "C++" => language_color(LanguageTag::Cpp, dark),
        "SQL" => language_color(LanguageTag::Sql, dark),
        "JSON" => language_color(LanguageTag::Json, dark),
        "YAML" => language_color(LanguageTag::Yaml, dark),
        "HTML" => language_color(LanguageTag::Html, dark),
        "CSS" => language_color(LanguageTag::Css, dark),
        "MD" => language_color(LanguageTag::Markdown, dark),
        "TOML" => language_color(LanguageTag::Toml, dark),
        "PARAM" => {
            if dark {
                rgb(0x93c5fd).into()
            } else {
                rgb(0x1d4ed8).into()
            }
        }
        "INFO" => {
            if dark {
                rgb(0x7dd3fc).into()
            } else {
                rgb(0x0369a1).into()
            }
        }
        "K8S" => {
            if dark {
                rgb(0x60a5fa).into()
            } else {
                rgb(0x1d4ed8).into()
            }
        }
        "DOCKER" => {
            if dark {
                rgb(0x38bdf8).into()
            } else {
                rgb(0x0284c7).into()
            }
        }
        "TF" => {
            if dark {
                rgb(0xc084fc).into()
            } else {
                rgb(0x7c3aed).into()
            }
        }
        "ANSIBLE" => {
            if dark {
                rgb(0xfb7185).into()
            } else {
                rgb(0xbe123c).into()
            }
        }
        "JWT" => {
            if dark {
                rgb(0xfbbf24).into()
            } else {
                rgb(0xb45309).into()
            }
        }
        "IP" => {
            if dark {
                rgb(0x5eead4).into()
            } else {
                rgb(0x0f766e).into()
            }
        }
        "ENV" => {
            if dark {
                rgb(0xa78bfa).into()
            } else {
                rgb(0x6d28d9).into()
            }
        }
        "PATH" => {
            if dark {
                rgb(0x93c5fd).into()
            } else {
                rgb(0x1d4ed8).into()
            }
        }
        "URL" => {
            if dark {
                rgb(0x5eead4).into()
            } else {
                rgb(0x0f766e).into()
            }
        }
        "MULTI" => {
            if dark {
                rgb(0xfde047).into()
            } else {
                rgb(0xa16207).into()
            }
        }
        "LONG" => {
            if dark {
                rgb(0xfdba74).into()
            } else {
                rgb(0xc2410c).into()
            }
        }
        _ => {
            if dark {
                rgb(0xd1d5db).into()
            } else {
                rgb(0x4b5563).into()
            }
        }
    }
}
