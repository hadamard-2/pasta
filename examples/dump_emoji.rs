//! Prints every emoji glyph the `emojis` crate yields, one per line. Used by
//! `scripts/generate_emoji_pngs.py` to key the bundled emoji PNGs by the exact
//! same corpus the launcher iterates at runtime.
//!
//!     cargo run --example dump_emoji > corpus.txt
fn main() {
    for emoji in emojis::iter() {
        println!("{}", emoji.as_str());
    }
}
