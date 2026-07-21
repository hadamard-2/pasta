# Emoji image attribution

The PNGs under `assets/emoji/png/` are used by the Linux emoji picker, which
renders each glyph from a bundled image instead of relying on the system's
color-emoji font (cosmic-text/GPUI on Linux mis-render many color emoji — see
`docs/` and the render path in `src/app/view.rs`).

## Sources

- **Emoji artwork — Noto Color Emoji**, © Google LLC, licensed under the SIL Open Font License 1.1. Full text in [`NOTO-EMOJI-OFL-LICENSE.txt`](./NOTO-EMOJI-OFL-LICENSE.txt). Upstream: https://github.com/googlefonts/noto-emoji
- **Flag artwork** — from noto-emoji's `third_party/region-flags`, sourced from Wikimedia Commons and in the public domain (or otherwise exempt from copyright).

## How the PNGs were generated

One-time, offline, by `scripts/generate_emoji_pngs.py`:

1. The exact glyph corpus is the `emojis` crate's `emojis::iter()` (the same set the launcher iterates at runtime), so every filename matches the key the render path computes.
2. Each glyph is keyed by its codepoints in lowercase hex joined with `-` (e.g. `1f1fa-1f1f8.png` for 🇺🇸), which is exactly what `emoji_tile_glyph` derives from `glyph.chars()`.
3. Non-flag glyphs are taken from noto-emoji `png/128` and downscaled to 72px. Flags aren't shipped as PNGs upstream, so they're rasterized from the waved flag SVGs (`third_party/region-flags/waved-svg`) with ImageMagick.

To regenerate (e.g. after an `emojis` crate bump), see the script header.
