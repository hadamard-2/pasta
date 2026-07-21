#!/usr/bin/env python3
"""Regenerate the bundled emoji PNGs under assets/emoji/png/.

The Linux emoji picker renders each glyph from a bundled Noto Color Emoji image
rather than the system color-emoji font (which cosmic-text/GPUI mis-render on
Linux). This script produces one 72px PNG per emoji in the launcher's corpus,
named by the glyph's codepoints in lowercase hex joined with '-' — the exact key
`emoji_tile_glyph` derives from `glyph.chars()` in src/app/view.rs.

Prerequisites:
  1. The glyph corpus, dumped from the crate the launcher actually uses:
         cargo run --example dump_emoji > corpus.txt
  2. A checkout of googlefonts/noto-emoji (blobless sparse is enough):
         git clone --depth 1 --filter=blob:none --sparse \\
             https://github.com/googlefonts/noto-emoji.git
         cd noto-emoji && git sparse-checkout set png/128 third_party
  3. ImageMagick (`convert`) on PATH — used to rasterize the waved flag SVGs,
     which upstream ships only as SVG (not in png/128).
  4. Pillow (`pip install pillow`).

Usage:
  python3 scripts/generate_emoji_pngs.py <corpus.txt> <noto-emoji-dir>

Coverage should be 100%; the printed audit breaks misses down by kind so a
whole category silently falling back to text rendering is caught here.
"""
import os
import subprocess
import sys
from PIL import Image

if len(sys.argv) != 3:
    sys.exit(__doc__)
CORPUS, NOTO_ROOT = sys.argv[1], sys.argv[2]
NOTO = os.path.join(NOTO_ROOT, "png/128")
WAVED = os.path.join(NOTO_ROOT, "third_party/region-flags/waved-svg")
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO, "assets/emoji/png")
SIZE = 72

os.makedirs(OUT, exist_ok=True)
glyphs = open(CORPUS, encoding="utf-8").read().splitlines()


def key_of(g):
    return "-".join(f"{ord(c):x}" for c in g)


def noto_pngs(g):
    """Noto filename candidates: FE0F stripped, codepoints zero-padded to 4."""
    cps = [ord(c) for c in g]
    stripped = [c for c in cps if c != 0xFE0F]
    return [
        "emoji_u" + "_".join(f"{c:04x}" for c in seq) + ".png"
        for seq in (stripped, cps)
    ]


def waved_svg(g):
    stripped = [ord(c) for c in g if ord(c) != 0xFE0F]
    return "emoji_u" + "_".join(f"{c:04x}" for c in stripped) + ".svg"


def fit_square(img, size):
    src = img.convert("RGBA")
    src.thumbnail((size, size), Image.LANCZOS)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas.paste(src, ((size - src.size[0]) // 2, (size - src.size[1]) // 2), src)
    return canvas


def kind(g):
    cps = [ord(c) for c in g]
    if any(c == 0x200D for c in cps):
        return "zwj"
    if any(0x1F1E6 <= c <= 0x1F1FF for c in cps):
        return "flag"
    if any(c == 0x20E3 for c in cps):
        return "keycap"
    return "compound" if len([c for c in cps if c != 0xFE0F]) > 1 else "single"


matched, missed = 0, []
by_kind, by_kind_hit = {}, {}
for g in glyphs:
    k = kind(g)
    by_kind[k] = by_kind.get(k, 0) + 1
    out_path = os.path.join(OUT, key_of(g) + ".png")

    raster = next((p for c in noto_pngs(g)
                   if os.path.exists(p := os.path.join(NOTO, c))), None)
    svg = os.path.join(WAVED, waved_svg(g))
    if raster is not None:
        fit_square(Image.open(raster), SIZE).save(out_path, "PNG")
    elif os.path.exists(svg):
        subprocess.run(
            ["convert", "-background", "none", svg, "-resize", f"{SIZE}x{SIZE}",
             "-gravity", "center", "-extent", f"{SIZE}x{SIZE}", out_path],
            check=True,
        )
    else:
        missed.append(g)
        continue
    matched += 1
    by_kind_hit[k] = by_kind_hit.get(k, 0) + 1

total = len(glyphs)
print(f"corpus: {total}   matched: {matched} ({100 * matched / total:.1f}%)   "
      f"missed: {len(missed)}")
for k in sorted(by_kind):
    print(f"  {k:<10} {by_kind_hit.get(k, 0):>4}/{by_kind[k]}")
if missed:
    print("\nMISSED (would fall back to text rendering):")
    for g in missed:
        print("  ", key_of(g), f"({kind(g)})")
    sys.exit(1)
