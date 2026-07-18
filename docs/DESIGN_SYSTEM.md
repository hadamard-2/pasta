# Pasta UI Design System

A Raycast-faithful visual language for the Pasta clipboard manager, written to be handed directly to a coding agent. Values map onto the existing code: the `Palette` struct in `src/ui/palette.rs`, the constants in `src/main.rs`, and the render tree in `src/app/view.rs`.

The overriding goal: Pasta should read as a quiet, dense, keyboard-first list. Almost none of the "Raycast feel" is color; it is density, restraint, and structure. If you get the grays wrong but the structure right, it still reads as Raycast. The reverse does not hold.

---

## 1. The five load-bearing decisions

These matter more than any single token. Do these and the app is 80% there.

1. **Rows are one line tall, not cards.** Today `RESULT_ROW_HEIGHT = 118.0`. Target is `40.0`. This is the single biggest change. A 118px row showing three lines of a code preview is what makes the app feel like a file manager; a 40px single-line row is what makes it feel like a launcher.
2. **Move the preview out of the row and into a detail pane.** The reason the row is 118px is that it is trying to show a multi-line preview inline. Raycast solves this with a split view: a narrow list of single-line rows on the left, a detail pane on the right showing the full content plus metadata of the selected row. This resolves the tension instead of fighting it. See section 6.
3. **Selection is a flat filled rounded rect, inset from the pane edge, with no border.** The current code draws a border on both selected and unselected rows. Remove borders from rows entirely. Selection is fill only.
4. **Go neutral gray.** The current palette is heavily blue-tinted (`0x08131f`, `0x7dd3fc`, `0x93c5fd`). Raycast is a near-zero-chroma grayscale ladder. Strip the blue.
5. **The action bar is permanent.** A fixed bar along the bottom naming the primary action and an Actions (Cmd/Ctrl+K) entry. It is what makes the app feel keyboard-first before a key is pressed.

---

## 2. Color tokens

A neutral grayscale ladder. These are opaque values: since blur is off on GNOME, the window background should be solid rather than translucent. Keep the `surface_alpha` plumbing, but the default palette below assumes alpha 1.0 and leans on the border plus shadow for separation from the desktop.

### Dark mode (primary target)

Map each row directly to the matching `Palette` field.

| `Palette` field     | New value    | Role                                                          |
| ------------------- | ------------ | ------------------------------------------------------------- |
| `window_bg`         | `0x17171Aff` | Window canvas                                                 |
| `window_border`     | `0x2C2C30ff` | 1px outer hairline                                            |
| `title_text`        | `0xEDEDEFff` | Primary text (row titles, "PASTA")                            |
| `query_placeholder` | `0x6E6E73ff` | Search bar placeholder                                        |
| `query_active`      | `0xF5F5F7ff` | Typed query text                                              |
| `muted_text`        | `0x8A8A8Fff` | Hotkey hint, section headers                                  |
| `list_divider`      | `0x262629ff` | Full-bleed separators                                         |
| `row_text`          | `0xEDEDEFff` | Row title                                                     |
| `row_meta_text`     | `0x8A8A8Fff` | Timestamps, accessories                                       |
| `row_hover_bg`      | `0xFFFFFF0A` | Hover fill (white at ~4% alpha)                               |
| `selected_bg`       | `0x2A2A2Eff` | Selection fill                                                |
| `selected_border`   | `0x00000000` | None. Set fully transparent, or drop the border call entirely |

One extra surface is worth adding to the struct for the action bar, slightly lifted from the canvas:

```rust
pub(crate) action_bar_bg: gpui::Rgba,   // dark: 0x1C1C1Fff
pub(crate) keycap_bg: gpui::Rgba,       // dark: 0x2A2A2Eff
pub(crate) keycap_text: gpui::Rgba,     // dark: 0x8A8A8Fff
```

### Light mode

| `Palette` field     | New value    |
| ------------------- | ------------ |
| `window_bg`         | `0xFFFFFFff` |
| `window_border`     | `0x00000014` |
| `title_text`        | `0x1D1D1Fff` |
| `query_placeholder` | `0x8E8E93ff` |
| `query_active`      | `0x000000ff` |
| `muted_text`        | `0x8E8E93ff` |
| `list_divider`      | `0x0000000D` |
| `row_text`          | `0x1D1D1Fff` |
| `row_meta_text`     | `0x8E8E93ff` |
| `row_hover_bg`      | `0x00000008` |
| `selected_bg`       | `0x00000010` |
| `selected_border`   | `0x00000000` |
| `action_bar_bg`     | `0xF5F5F5ff` |
| `keycap_bg`         | `0x0000000D` |
| `keycap_text`       | `0x8E8E93ff` |

### Type accent colors

The current `type_color` values are fully saturated. Raycast keeps type indicators muted and applies them only to the small leading icon, never to a chip background. Two options, in order of preference:

- **Preferred:** desaturate heavily and apply color only to the row's leading type icon. Keep the ladder recognizable but dim (roughly 40 to 55% saturation of the current values).
- **Acceptable:** keep the current hues for the type chips but move them to the detail pane, not the list row. The list row stays monochrome.

Suggested muted dark-mode set:

| Type     | Current    | Muted      |
| -------- | ---------- | ---------- |
| Text     | `0x38bdf8` | `0x6E9ECF` |
| Code     | `0x34d399` | `0x5FB99A` |
| Command  | `0xfbbf24` | `0xCFA85F` |
| Password | `0xf472b6` | `0xCF7FA3` |

---

## 3. Window shell

In `src/platform/linux/mod.rs` the window is already set up well: `titlebar: None`, `kind: WindowKind::PopUp`, `window_background: WindowBackgroundAppearance::Transparent`. Keep all of that. The transparent background plus your own drawn surface is correct; it is how you get custom rounded corners.

Changes:

- **Corner radius:** the panel uses `rounded_2xl` (16px). Raycast is closer to 10px. Change to `rounded_lg` (8px) or an explicit `rounded(px(10.0))`.
- **Window size:** current `860 x 560`. That is fine for a split view. If you keep it list-only, drop to roughly `750 x 475`.
- **Shadow:** keep `shadow_xl` on the panel. It is doing real work now that the background is opaque.
- **Outer padding:** the content uses `px_4 py_3`. Reduce the vertical component; the search bar should sit close to the top edge. `px_3 pt_2` is a good starting point, with the list flush to the separator below the search bar.

---

## 4. Search bar

The top region is the search field itself; there is no separate titlebar. Structure top to bottom:

- A thin header line with `PASTA` on the left (`title_text`, 11px) and the hotkey hint on the right (`muted_text`, 11px). Keep this; it is understated and correct. Consider dropping it entirely to match Raycast exactly, which puts nothing above the search field.
- The search input: 15px text (`text_base` to `text_lg`), `query_placeholder` when empty, `query_active` when typed. Line height around 30px. No border, no background fill; it sits directly on the canvas.
- A single full-bleed `list_divider` separator directly beneath the search field. This is the only separator in the top half of the window.

---

## 5. List rows

This is where the density lives. Target anatomy for a single row at `40px` tall:

```
[8px inset] [type icon 15px] [gap 10px] [title, single line, ellipsis] [flex] [timestamp 11px] [8px inset]
```

Concrete rules:

- **Height:** `RESULT_ROW_HEIGHT = 40.0` in `src/main.rs`. The row wrapper keeps `py(px(2.0))` so selection fills do not touch each other.
- **Horizontal inset:** the selection and hover fills inset 8px from the pane edge (`mx_2` on the fill, or padding on the parent and the fill going edge to edge inside it). This inset is what makes selection read as a pill rather than a bar.
- **Radius:** `rounded_md` (6px) on the fill.
- **No border on any row.** Remove the `border_1` and `border_color` calls in `render_result_row` for both selected and unselected states.
- **Title:** `row_text`, 13px, `FontWeight::NORMAL`, single line, ellipsis on overflow (`whitespace_nowrap`, `overflow_hidden`, `text_ellipsis`). The current row uses `whitespace_normal` with a 72px max height; switch to single-line truncation.
- **Leading icon:** 15px, colored per the muted type set. This replaces the type chip in the list.
- **Timestamp:** `row_meta_text`, 11px, right-aligned.
- **Tags and chips:** move to the detail pane. A dense list row shows at most the title, the type icon, and the timestamp. Secret state (`LOCKED` / `OPEN 12s`) is the one exception worth keeping inline, as a single small pill.

### Section headers

Group rows under muted headers ("Today", "Yesterday", "Last 7 days"). Header style: `muted_text`, 11px, roughly 24px of row space, small left inset matching the row content. This is cheap to add and does a lot for the Raycast feel.

---

## 6. Detail pane (recommended)

This is the bigger architectural change and the one that earns the density. Split the content region horizontally:

- **Left list pane:** roughly 44% width, the single-line rows from section 5.
- **Vertical separator:** 1px `list_divider`.
- **Right detail pane:** the full preview of the selected item plus a metadata block.

The detail pane is where the current 118px row's content actually belongs:

- **Preview:** the full clipboard content, syntax-highlighted, monospace, scrollable. Your `syntax_styled_text` and `src/ui/syntax.rs` already do this; point them at the detail pane instead of the row.
- **Metadata block:** a small key/value table at the bottom of the pane. Label in `muted_text`, value in `row_text`, 11px, rows about 20px tall. Good fields: application, type, character count, copied-at, tags, bowl.
- **Tags and chips** live here, wrapping under the metadata.

If the detail pane is too big a lift for a first pass, the fallback is: keep it list-only, single-line 40px rows, and show the preview in a transient panel on a keypress. But the split view is the version that feels finished.

---

## 7. Action bar

A permanent bar pinned to the bottom of the window.

- **Height:** 40px.
- **Background:** `action_bar_bg` (one step lifted from the canvas).
- **Top border:** 1px `list_divider`.
- **Left:** a small app glyph (16px) plus a context label in `muted_text`, 12px (for example the current view name, "Clipboard history").
- **Right:** the primary action named in `row_text` 12px, followed by its keycap; then a thin vertical divider; then "Actions" and its `Ctrl+K` keycap.

### Keycap pills

Reused throughout (action bar, hints, the transforms menu):

- Background `keycap_bg`, text `keycap_text`, 11px, `rounded_sm` (4px), padding roughly `px(6.0) py(2.0)`.
- Symbols: `↵` for enter, `⌘K` on macOS and `Ctrl+K` on Linux, `Tab` for transforms.

---

## 8. Typography scale

Two weights only: `LIGHT` for large display text if you want it, `NORMAL` for everything functional. Avoid heavier weights; they fight the quiet aesthetic.

| Role                      | Size | Weight | Color                       |
| ------------------------- | ---- | ------ | --------------------------- |
| Search query              | 15px | NORMAL | `query_active`              |
| Row title                 | 13px | NORMAL | `row_text`                  |
| Row timestamp / accessory | 11px | NORMAL | `row_meta_text`             |
| Section header            | 11px | NORMAL | `muted_text`                |
| Detail preview            | 12px | NORMAL | `row_text`, monospace       |
| Metadata label / value    | 11px | NORMAL | `muted_text` / `row_text`   |
| Keycap                    | 11px | NORMAL | `keycap_text`               |
| App header / hotkey hint  | 11px | NORMAL | `title_text` / `muted_text` |

Font: Raycast uses SF Pro. Inter is the standard free substitute and is very close at UI sizes. Embed it rather than depending on the system font, and keep the existing monospace for previews.

---

## 9. Spacing and radius reference

| Token                 | Value | Used for                            |
| --------------------- | ----- | ----------------------------------- |
| Window radius         | 10px  | Panel outer corners                 |
| Selection radius      | 6px   | Row selection and hover fill        |
| Keycap radius         | 4px   | Keycap pills, small chips           |
| Row height            | 40px  | List rows                           |
| Section header height | 24px  | Group labels                        |
| Action bar height     | 40px  | Bottom bar                          |
| Search bar height     | ~52px | Top region including header line    |
| Row horizontal inset  | 8px   | Selection fill inset from pane edge |
| Row content gap       | 10px  | Between icon and title              |

---

## 10. What to remove

Explicitly, so the agent does not preserve things that read as clutter:

- Per-row borders (selected and unselected).
- Multi-line inline previews in the list.
- Saturated type-chip backgrounds in the list.
- Blue tint across the whole palette.
- The 16px window radius.
- Any translucency on the window background while blur is unavailable.

---

## 11. Suggested change order

For a coding agent, smallest-to-largest so each step is independently verifiable:

1. Swap the palette values (section 2). Pure data change in `src/ui/palette.rs`.
2. Set `RESULT_ROW_HEIGHT = 40.0` and rewrite `render_result_row` to a single-line layout with truncation, no border, inset selection fill (sections 5, 10).
3. Change window radius to 10px and reduce outer padding (section 3).
4. Add section headers (section 5).
5. Add the permanent action bar and keycap component (section 7).
6. Split the content region into list plus detail pane, and relocate the preview, metadata, and tags into it (section 6).

Steps 1 through 3 alone will already make it recognizably Raycast. Steps 4 through 6 are what make it feel finished.
