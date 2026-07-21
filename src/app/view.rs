use super::actions::{
    expand_candidates_with_splits, has_structured_parameter_candidates,
    parameter_clickable_candidates,
};
use super::query_input::TextInputElement;
use super::state::CachedRowPresentation;
use crate::*;
use gpui::{AnyElement, StatefulInteractiveElement, canvas, hsla, size, svg};

/// Emoji tiles are laid out `EMOJI_GRID_COLUMNS` per row, each taking an
/// equal relative share of the row's width — sidesteps needing to measure
/// the panel's actual pixel width to decide how many tiles fit.
pub(crate) const EMOJI_GRID_COLUMNS: usize = 10;

/// The visual for a single emoji tile. On Linux the glyph is rendered from its
/// bundled Noto Color Emoji PNG (keyed by codepoints joined with `-`, served by
/// `Assets`), which sidesteps cosmic-text/GPUI's color-emoji font-fallback
/// limitations; a missing image falls back to the text glyph. Other platforms
/// render the text glyph directly, since they draw color emoji natively.
fn emoji_tile_glyph(glyph: &str) -> AnyElement {
    #[cfg(target_os = "linux")]
    {
        let key = glyph
            .chars()
            .map(|c| format!("{:x}", c as u32))
            .collect::<Vec<_>>()
            .join("-");
        let fallback_glyph = glyph.to_owned();
        img(format!("emoji/{key}.png"))
            .w(px(40.0))
            .h(px(40.0))
            .object_fit(ObjectFit::Contain)
            .with_fallback(move || {
                div()
                    .text_size(px(28.0))
                    .child(fallback_glyph.clone())
                    .into_any_element()
            })
            .into_any_element()
    }
    #[cfg(not(target_os = "linux"))]
    {
        div()
            .text_size(px(28.0))
            .child(glyph.to_owned())
            .into_any_element()
    }
}

impl Render for LauncherView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_pending_text_input_focus(window);
        let palette = palette_for(self.surface_alpha);
        let info_editor_open = self.info_editor_target_id.is_some();
        let tag_editor_open = self.tag_editor_target_id.is_some();
        let bowl_editor_open = self.bowl_editor_target_id.is_some();
        let parameter_editor_open = self.parameter_editor_target_id.is_some();
        let parameter_fill_open = self.parameter_fill_target_id.is_some();
        let transform_menu_open = self.transform_menu_open;
        let query_input_enabled = self.query_input_enabled();
        let query_focus_handle = self.text_input_focus_handle(TextInputTarget::Query);
        let query_focused = query_focus_handle.is_focused(window);

        let results = if self.items.is_empty() {
            div()
                .id("results-list")
                .w_full()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(palette.muted_text)
                .text_sm()
                .child("Nothing copied yet.")
                .into_any_element()
        } else {
            uniform_list(
                "results-list",
                self.items.len(),
                cx.processor(move |this, range: Range<usize>, _window, cx| {
                    let mut rows = Vec::with_capacity(range.end.saturating_sub(range.start));
                    for ix in range {
                        if let (Some(item), Some(row_data)) =
                            (this.items.get(ix), this.row_presentations.get(ix))
                        {
                            rows.push(this.render_result_row(
                                ix,
                                item,
                                row_data,
                                palette,
                                info_editor_open,
                                tag_editor_open,
                                bowl_editor_open,
                                parameter_editor_open,
                                parameter_fill_open,
                                transform_menu_open,
                                cx,
                            ));
                        }
                    }
                    rows
                }),
            )
            .w_full()
            .h_full()
            .track_scroll(self.results_scroll.clone())
            .into_any_element()
        };

        let mut panel = div()
            .size_full()
            .font_family(self.ui_font_family.clone())
            .font_weight(FontWeight::LIGHT)
            .opacity(self.transition_alpha)
            .bg(palette.window_bg)
            .border_1()
            .border_color(palette.window_border)
            .rounded(px(10.0))
            .overflow_hidden()
            .flex()
            .flex_col();
        if self.transition_target > 0.0 && self.transition_alpha > 0.35 {
            panel = panel.shadow_xl();
        }

        let mut content =
            div()
                .flex_1()
                .min_h(px(0.0))
                .px_3()
                .pt_2()
                .flex()
                .flex_col()
                .gap_2()
                .child({
                    let mut query_container = div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        // Emoji mode's back button sits flush with the divider
                        // below instead of at the search field's usual inset.
                        .pl(if self.emoji_search_active {
                            px(0.0)
                        } else {
                            px(8.0)
                        })
                        .pr(px(8.0))
                        .pt(px(4.0))
                        .pb(px(2.0))
                        .rounded_md()
                        .line_height(px(30.0))
                        .text_base()
                        .font_weight(FontWeight::NORMAL);

                    if query_input_enabled {
                        query_container = query_container
                            .key_context("PastaTextInput")
                            .track_focus(&query_focus_handle)
                            .cursor(CursorStyle::IBeam)
                            .on_action(cx.listener(Self::query_backspace))
                            .on_action(cx.listener(Self::query_delete_word_backward))
                            .on_action(cx.listener(Self::query_select_all))
                            .on_action(cx.listener(Self::query_home))
                            .on_action(cx.listener(Self::query_end))
                            .on_action(cx.listener(Self::query_show_character_palette))
                            .on_action(cx.listener(Self::query_paste))
                            .on_action(cx.listener(Self::query_cut))
                            .on_action(cx.listener(Self::query_copy))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event, window, cx| {
                                    this.text_input_on_mouse_down(
                                        TextInputTarget::Query,
                                        event,
                                        window,
                                        cx,
                                    );
                                }),
                            )
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, event, window, cx| {
                                    this.text_input_on_mouse_up(
                                        TextInputTarget::Query,
                                        event,
                                        window,
                                        cx,
                                    );
                                }),
                            )
                            .on_mouse_up_out(
                                MouseButton::Left,
                                cx.listener(|this, event, window, cx| {
                                    this.text_input_on_mouse_up(
                                        TextInputTarget::Query,
                                        event,
                                        window,
                                        cx,
                                    );
                                }),
                            )
                            .on_mouse_move(cx.listener(|this, event, window, cx| {
                                this.text_input_on_mouse_move(
                                    TextInputTarget::Query,
                                    event,
                                    window,
                                    cx,
                                );
                            }));

                        // Left/Right (plain or Shift-extended) normally move/extend
                        // the text cursor, but in emoji search mode they move the
                        // grid selection instead (see `handle_emoji_search_keystroke`)
                        // — binding both would fire the cursor move and the grid
                        // move on every press.
                        if !self.emoji_search_active {
                            query_container = query_container
                                .on_action(cx.listener(Self::query_left))
                                .on_action(cx.listener(Self::query_right))
                                .on_action(cx.listener(Self::query_select_left))
                                .on_action(cx.listener(Self::query_select_right));
                        }
                    }

                    // The search field sits directly on the canvas — no border, no
                    // background fill, focused or not (section 4 of the design system).
                    let _ = query_focused;

                    if self.emoji_search_active {
                        query_container = query_container.child(
                            div()
                                .id("emoji-mode-back")
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(32.0))
                                .h(px(32.0))
                                .rounded_md()
                                .border_1()
                                .border_color(palette.window_border)
                                .bg(palette.row_hover_bg)
                                .cursor_pointer()
                                .hover({
                                    let selected_bg = palette.selected_bg;
                                    move |style| style.bg(selected_bg)
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel_emoji_search(cx);
                                }))
                                .child(
                                    svg()
                                        .path("icons/chevron-left.svg")
                                        .size(px(18.0))
                                        .text_color(palette.title_text),
                                ),
                        );
                    } else {
                        query_container = query_container.child(
                            svg()
                                .path("icons/search.svg")
                                .size(px(14.0))
                                .flex_shrink_0()
                                .text_color(palette.muted_text),
                        );
                    }

                    div().w_full().child(query_container.child(
                        div().flex_1().min_w(px(0.0)).child(TextInputElement::new(
                            cx.entity(),
                            TextInputTarget::Query,
                            if self.emoji_search_active {
                                "Search emoji"
                            } else {
                                "Pasta"
                            },
                            palette,
                            query_input_enabled,
                        )),
                    ))
                });
        if !self.tag_search_suggestions.is_empty()
            && query_input_enabled
            && !self.emoji_search_active
        {
            content = content.child(self.render_tag_search_suggestions(palette, cx));
        }

        if self.showing_emoji_affordance() {
            content = content.child(
                div()
                    .id("emoji-search-affordance")
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(8.0))
                    .py(px(6.0))
                    .rounded_md()
                    .border_1()
                    .border_color(palette.accent)
                    .cursor_pointer()
                    .hover({
                        let row_hover = palette.row_hover_bg;
                        move |style| style.bg(row_hover)
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.enter_emoji_search_mode(cx);
                    }))
                    .child(
                        div()
                            .flex_none()
                            .w(px(15.0))
                            .flex()
                            .justify_center()
                            .text_size(px(14.0))
                            .text_color(palette.accent)
                            .child("🙂"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(14.0))
                            .text_color(palette.title_text)
                            .child("Emoji Search"),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(palette.muted_text)
                            .child("Enter"),
                    ),
            );
        }

        if let Some(item_id) = self.info_editor_target_id {
            let info_editor_focus_handle =
                self.text_input_focus_handle(TextInputTarget::InfoEditor);
            let info_editor_focused = info_editor_focus_handle.is_focused(window);
            let mut info_input = div()
                .w_full()
                .mt_1()
                .px_1()
                .rounded_md()
                .line_height(px(24.0))
                .text_sm()
                .font_weight(FontWeight::NORMAL)
                .key_context("PastaTextInput")
                .track_focus(&info_editor_focus_handle)
                .cursor(CursorStyle::IBeam)
                .on_action(cx.listener(Self::query_backspace))
                .on_action(cx.listener(Self::query_delete_word_backward))
                .on_action(cx.listener(Self::query_left))
                .on_action(cx.listener(Self::query_right))
                .on_action(cx.listener(Self::query_select_left))
                .on_action(cx.listener(Self::query_select_right))
                .on_action(cx.listener(Self::query_select_all))
                .on_action(cx.listener(Self::query_home))
                .on_action(cx.listener(Self::query_end))
                .on_action(cx.listener(Self::query_show_character_palette))
                .on_action(cx.listener(Self::query_paste))
                .on_action(cx.listener(Self::query_cut))
                .on_action(cx.listener(Self::query_copy))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event, window, cx| {
                        this.text_input_on_mouse_down(
                            TextInputTarget::InfoEditor,
                            event,
                            window,
                            cx,
                        );
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, event, window, cx| {
                        this.text_input_on_mouse_up(TextInputTarget::InfoEditor, event, window, cx);
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, event, window, cx| {
                        this.text_input_on_mouse_up(TextInputTarget::InfoEditor, event, window, cx);
                    }),
                )
                .on_mouse_move(cx.listener(|this, event, window, cx| {
                    this.text_input_on_mouse_move(TextInputTarget::InfoEditor, event, window, cx);
                }));

            if info_editor_focused {
                info_input = info_input
                    .bg(scale_alpha(
                        palette.selected_bg,
                        if palette.dark { 0.95 } else { 0.75 },
                    ))
                    .border_1()
                    .border_color(palette.selected_border);
            }

            content = content.child(
                div()
                    .w_full()
                    .p_2()
                    .bg(scale_alpha(
                        palette.row_hover_bg,
                        if palette.dark { 0.95 } else { 1.0 },
                    ))
                    .border_1()
                    .border_color(palette.selected_border)
                    .rounded_lg()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(palette.title_text)
                                    .child(format!("Snippet Info • Snippet #{item_id}")),
                            ),
                    )
                    .child(info_input.child(TextInputElement::new(
                        cx.entity(),
                        TextInputTarget::InfoEditor,
                        "Add info…",
                        palette,
                        true,
                    )))
                    .child(
                        div()
                            .w_full()
                            .mt_1()
                            .text_xs()
                            .text_color(palette.muted_text)
                            .child(if cfg!(target_os = "macos") {
                                "⌘V paste"
                            } else {
                                "Ctrl+V paste"
                            }),
                    ),
            );
        }

        if let Some(item_id) = self.tag_editor_target_id {
            let tag_editor_focus_handle = self.text_input_focus_handle(TextInputTarget::TagEditor);
            let tag_editor_focused = tag_editor_focus_handle.is_focused(window);
            let mut tag_input = div()
                .w_full()
                .mt_1()
                .px_1()
                .rounded_md()
                .line_height(px(24.0))
                .text_sm()
                .font_weight(FontWeight::NORMAL)
                .key_context("PastaTextInput")
                .track_focus(&tag_editor_focus_handle)
                .cursor(CursorStyle::IBeam)
                .on_action(cx.listener(Self::query_backspace))
                .on_action(cx.listener(Self::query_delete_word_backward))
                .on_action(cx.listener(Self::query_left))
                .on_action(cx.listener(Self::query_right))
                .on_action(cx.listener(Self::query_select_left))
                .on_action(cx.listener(Self::query_select_right))
                .on_action(cx.listener(Self::query_select_all))
                .on_action(cx.listener(Self::query_home))
                .on_action(cx.listener(Self::query_end))
                .on_action(cx.listener(Self::query_show_character_palette))
                .on_action(cx.listener(Self::query_paste))
                .on_action(cx.listener(Self::query_cut))
                .on_action(cx.listener(Self::query_copy))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event, window, cx| {
                        this.text_input_on_mouse_down(
                            TextInputTarget::TagEditor,
                            event,
                            window,
                            cx,
                        );
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, event, window, cx| {
                        this.text_input_on_mouse_up(TextInputTarget::TagEditor, event, window, cx);
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, event, window, cx| {
                        this.text_input_on_mouse_up(TextInputTarget::TagEditor, event, window, cx);
                    }),
                )
                .on_mouse_move(cx.listener(|this, event, window, cx| {
                    this.text_input_on_mouse_move(TextInputTarget::TagEditor, event, window, cx);
                }));
            if tag_editor_focused {
                tag_input = tag_input
                    .bg(scale_alpha(
                        palette.selected_bg,
                        if palette.dark { 0.95 } else { 0.75 },
                    ))
                    .border_1()
                    .border_color(palette.selected_border);
            }
            let title = if self.tag_editor_mode == TagEditorMode::Add {
                "Add Custom Tags"
            } else {
                "Remove Tags"
            };

            content = content.child(
                div()
                    .w_full()
                    .p_2()
                    .bg(scale_alpha(
                        palette.row_hover_bg,
                        if palette.dark { 0.95 } else { 1.0 },
                    ))
                    .border_1()
                    .border_color(palette.selected_border)
                    .rounded_lg()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(palette.title_text)
                                    .child(format!("{title} • Snippet #{item_id}")),
                            ),
                    )
                    .child(tag_input.child(TextInputElement::new(
                        cx.entity(),
                        TextInputTarget::TagEditor,
                        "tag1,tag2",
                        palette,
                        true,
                    )))
                    .child(
                        div()
                            .w_full()
                            .mt_1()
                            .text_xs()
                            .text_color(palette.muted_text)
                            .child(if cfg!(target_os = "macos") {
                                "comma-separated • ⌘V"
                            } else {
                                "comma-separated • Ctrl+V"
                            }),
                    ),
            );
        }

        if let Some(item_id) = self.bowl_editor_target_id {
            let bowl_editor_focus_handle =
                self.text_input_focus_handle(TextInputTarget::BowlEditor);
            let bowl_editor_focused = bowl_editor_focus_handle.is_focused(window);
            let mut bowl_input = div()
                .w_full()
                .mt_1()
                .px_1()
                .rounded_md()
                .line_height(px(24.0))
                .text_sm()
                .font_weight(FontWeight::NORMAL)
                .key_context("PastaTextInput")
                .track_focus(&bowl_editor_focus_handle)
                .cursor(CursorStyle::IBeam)
                .on_action(cx.listener(Self::query_backspace))
                .on_action(cx.listener(Self::query_delete_word_backward))
                .on_action(cx.listener(Self::query_left))
                .on_action(cx.listener(Self::query_right))
                .on_action(cx.listener(Self::query_select_left))
                .on_action(cx.listener(Self::query_select_right))
                .on_action(cx.listener(Self::query_select_all))
                .on_action(cx.listener(Self::query_home))
                .on_action(cx.listener(Self::query_end))
                .on_action(cx.listener(Self::query_show_character_palette))
                .on_action(cx.listener(Self::query_paste))
                .on_action(cx.listener(Self::query_cut))
                .on_action(cx.listener(Self::query_copy))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event, window, cx| {
                        this.text_input_on_mouse_down(
                            TextInputTarget::BowlEditor,
                            event,
                            window,
                            cx,
                        );
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, event, window, cx| {
                        this.text_input_on_mouse_up(TextInputTarget::BowlEditor, event, window, cx);
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, event, window, cx| {
                        this.text_input_on_mouse_up(TextInputTarget::BowlEditor, event, window, cx);
                    }),
                )
                .on_mouse_move(cx.listener(|this, event, window, cx| {
                    this.text_input_on_mouse_move(TextInputTarget::BowlEditor, event, window, cx);
                }));
            if bowl_editor_focused {
                bowl_input = bowl_input
                    .bg(scale_alpha(
                        palette.selected_bg,
                        if palette.dark { 0.95 } else { 0.75 },
                    ))
                    .border_1()
                    .border_color(palette.selected_border);
            }

            let mut bowl_panel = div()
                .w_full()
                .p_2()
                .bg(scale_alpha(
                    palette.row_hover_bg,
                    if palette.dark { 0.95 } else { 1.0 },
                ))
                .border_1()
                .border_color(palette.selected_border)
                .rounded_lg()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_sm()
                                .text_color(palette.title_text)
                                .child(format!("Assign Bowl • Snippet #{item_id}")),
                        ),
                )
                .child(bowl_input.child(TextInputElement::new(
                    cx.entity(),
                    TextInputTarget::BowlEditor,
                    "BOWL-NAME",
                    palette,
                    true,
                )));

            if !self.bowl_editor_suggestions.is_empty() {
                let mut chips = div().w_full().mt_1().flex().flex_row().flex_wrap().gap_1();
                for (ix, suggestion) in self.bowl_editor_suggestions.iter().enumerate() {
                    let is_primary = ix == 0;
                    let chip_bg = if is_primary {
                        scale_alpha(palette.selected_bg, if palette.dark { 0.92 } else { 0.72 })
                    } else {
                        scale_alpha(palette.row_hover_bg, if palette.dark { 0.9 } else { 1.0 })
                    };
                    let chip_border = if is_primary {
                        palette.selected_border
                    } else {
                        scale_alpha(palette.window_border, if palette.dark { 0.84 } else { 0.9 })
                    };
                    let chip_text = if is_primary {
                        palette.title_text
                    } else {
                        palette.muted_text
                    };
                    let suggestion_owned = suggestion.clone();
                    chips = chips.child(
                        div()
                            .id(("bowl-editor-suggestion", ix))
                            .text_xs()
                            .text_color(chip_text)
                            .bg(chip_bg)
                            .border_1()
                            .border_color(chip_border)
                            .rounded_md()
                            .px_1()
                            .py(px(1.0))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.bowl_editor_input = suggestion_owned.clone();
                                let len = this.bowl_editor_input.len();
                                this.bowl_editor_input_state.selected_range = len..len;
                                this.bowl_editor_input_state.selection_reversed = false;
                                this.bowl_editor_input_state.marked_range = None;
                                this.bowl_editor_suggestions =
                                    this.storage.suggest_bowl_names(&this.bowl_editor_input, 6);
                                cx.notify();
                            }))
                            .child(suggestion.clone()),
                    );
                }
                bowl_panel = bowl_panel.child(chips);
            }

            let help_text = if self.bowl_editor_suggestions.is_empty() {
                if cfg!(target_os = "macos") {
                    "single bowl • blank = remove • ⌘V"
                } else {
                    "single bowl • blank = remove • Ctrl+V"
                }
            } else {
                "single bowl • Tab autocomplete • blank = remove"
            };
            bowl_panel = bowl_panel.child(
                div()
                    .w_full()
                    .mt_1()
                    .text_xs()
                    .text_color(palette.muted_text)
                    .child(help_text),
            );
            content = content.child(bowl_panel);
        }

        if let Some(item_id) = self.parameter_editor_target_id {
            if self.parameter_editor_stage == ParameterEditorStage::SelectValue {
                let item_content = self
                    .items
                    .iter()
                    .find(|entry| entry.id == item_id)
                    .map(|entry| entry.content.clone())
                    .unwrap_or_default();
                let has_structured_candidates = has_structured_parameter_candidates(&item_content);
                let candidates =
                    parameter_clickable_candidates(&item_content, self.parameter_editor_force_full);
                let candidates =
                    expand_candidates_with_splits(candidates, &self.parameter_editor_split_tokens);
                let auto_named_candidates =
                    has_structured_candidates && !self.parameter_editor_force_full;
                let mut token_picker = div().w_full().mt_1().flex().flex_row().flex_wrap().gap_1();
                for (range_ix, candidate) in candidates.into_iter().take(120).enumerate() {
                    if candidate.target.is_empty() {
                        continue;
                    }
                    let token = candidate.label;
                    let target = candidate.target;
                    let is_selected = self
                        .parameter_editor_selected_targets
                        .iter()
                        .any(|existing| existing == &target);
                    let chip_bg = if is_selected {
                        if palette.dark {
                            rgb(0x22d3ee)
                        } else {
                            rgb(0x0891b2)
                        }
                    } else {
                        scale_alpha(palette.row_hover_bg, if palette.dark { 0.92 } else { 1.0 })
                    };
                    let chip_border = if is_selected {
                        if palette.dark {
                            rgb(0x67e8f9)
                        } else {
                            rgb(0x0e7490)
                        }
                    } else {
                        scale_alpha(palette.window_border, if palette.dark { 0.85 } else { 1.0 })
                    };

                    token_picker = token_picker.child(
                        div()
                            .id(("parameter-token", range_ix as u64))
                            .text_xs()
                            .text_color(if is_selected {
                                if palette.dark {
                                    rgb(0x042f2e)
                                } else {
                                    rgb(0xffffff)
                                }
                            } else {
                                palette.row_text
                            })
                            .bg(chip_bg)
                            .border_1()
                            .border_color(chip_border)
                            .rounded_md()
                            .px_1()
                            .py(px(1.0))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                                let mods = event.modifiers();
                                let additive = if cfg!(target_os = "macos") {
                                    mods.platform
                                } else {
                                    mods.control
                                };
                                this.select_parameter_clickable_range(range_ix, additive, cx);
                            }))
                            .child(token),
                    );
                }

                let mut selector_header = div()
                    .w_full()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_sm().text_color(palette.title_text).child(
                        if auto_named_candidates {
                            format!("Select Parameters • Snippet #{item_id}")
                        } else if has_structured_candidates && self.parameter_editor_force_full {
                            format!("Full Parametrize • Snippet #{item_id}")
                        } else {
                            format!("Parametrize Snippet • Snippet #{item_id}")
                        },
                    ));

                let guided_active = has_structured_candidates && !self.parameter_editor_force_full;
                let full_active = self.parameter_editor_force_full || !has_structured_candidates;

                let guided_bg = if guided_active {
                    if palette.dark {
                        rgb(0x22d3ee)
                    } else {
                        rgb(0x0891b2)
                    }
                } else {
                    scale_alpha(palette.row_hover_bg, if palette.dark { 0.95 } else { 1.0 })
                };
                let guided_border = if guided_active {
                    if palette.dark {
                        rgb(0x67e8f9)
                    } else {
                        rgb(0x0e7490)
                    }
                } else {
                    scale_alpha(palette.window_border, if palette.dark { 0.85 } else { 1.0 })
                };
                let full_bg = if full_active {
                    if palette.dark {
                        rgb(0x22d3ee)
                    } else {
                        rgb(0x0891b2)
                    }
                } else {
                    scale_alpha(palette.row_hover_bg, if palette.dark { 0.95 } else { 1.0 })
                };
                let full_border = if full_active {
                    if palette.dark {
                        rgb(0x67e8f9)
                    } else {
                        rgb(0x0e7490)
                    }
                } else {
                    scale_alpha(palette.window_border, if palette.dark { 0.85 } else { 1.0 })
                };

                selector_header = selector_header.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .id(("parameter-mode-guided", item_id as u64))
                                .text_xs()
                                .text_color(if guided_active {
                                    if palette.dark {
                                        rgb(0x042f2e)
                                    } else {
                                        rgb(0xffffff)
                                    }
                                } else if has_structured_candidates {
                                    palette.row_text
                                } else {
                                    palette.muted_text
                                })
                                .bg(guided_bg)
                                .border_1()
                                .border_color(guided_border)
                                .rounded_md()
                                .px_1()
                                .py(px(1.0))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_parameter_editor_full_mode(false, cx);
                                }))
                                .child("Guided (g)"),
                        )
                        .child(
                            div()
                                .id(("parameter-mode-full", item_id as u64))
                                .text_xs()
                                .text_color(if full_active {
                                    if palette.dark {
                                        rgb(0x042f2e)
                                    } else {
                                        rgb(0xffffff)
                                    }
                                } else {
                                    palette.row_text
                                })
                                .bg(full_bg)
                                .border_1()
                                .border_color(full_border)
                                .rounded_md()
                                .px_1()
                                .py(px(1.0))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_parameter_editor_full_mode(true, cx);
                                }))
                                .child("Full (f)"),
                        ),
                );

                content = content.child(
                    div()
                        .w_full()
                        .p_2()
                        .bg(scale_alpha(
                            palette.row_hover_bg,
                            if palette.dark { 0.95 } else { 1.0 },
                        ))
                        .border_1()
                        .border_color(palette.selected_border)
                        .rounded_lg()
                        .child(selector_header)
                        .child(token_picker)
                        .child(
                            div()
                                .w_full()
                                .mt_1()
                                .text_xs()
                                .text_color(palette.muted_text)
                                .child(if self.parameter_editor_selected_targets.is_empty() {
                                    if auto_named_candidates {
                                        "pick one or more fields"
                                    } else {
                                        if cfg!(target_os = "macos") {
                                            "pick values • ⌘+click to split"
                                        } else {
                                            "pick values • Ctrl+click to split"
                                        }
                                    }
                                } else if auto_named_candidates {
                                    if cfg!(target_os = "macos") {
                                        "Enter saves • ⌘+click toggles"
                                    } else {
                                        "Enter saves • Ctrl+click toggles"
                                    }
                                } else {
                                    if cfg!(target_os = "macos") {
                                        "Enter then name • ⌘+click splits or toggles"
                                    } else {
                                        "Enter then name • Ctrl+click splits or toggles"
                                    }
                                }),
                        ),
                );
            } else {
                let parameter_name_focus_handle =
                    self.text_input_focus_handle(TextInputTarget::ParameterName);
                let mut name_rows = div().w_full().mt_1().flex().flex_col().gap_1();
                if self.parameter_editor_selected_targets.is_empty() {
                    name_rows = name_rows.child(
                        div()
                            .text_xs()
                            .text_color(palette.muted_text)
                            .child("No targets selected."),
                    );
                } else {
                    for (ix, target) in self.parameter_editor_selected_targets.iter().enumerate() {
                        let is_focus = ix == self.parameter_editor_name_focus_index;
                        let value = self
                            .parameter_editor_name_inputs
                            .get(ix)
                            .cloned()
                            .unwrap_or_default();
                        let value_display = if value.is_empty() {
                            "name".to_owned()
                        } else {
                            value
                        };
                        let value_color = if value_display == "name" {
                            palette.query_placeholder
                        } else {
                            palette.query_active
                        };
                        let mut name_input = div()
                            .w_full()
                            .mt_1()
                            .px_1()
                            .rounded_sm()
                            .line_height(px(22.0))
                            .text_sm()
                            .font_weight(FontWeight::NORMAL);
                        if is_focus {
                            name_input = name_input
                                .key_context("PastaTextInput")
                                .track_focus(&parameter_name_focus_handle)
                                .cursor(CursorStyle::IBeam)
                                .on_action(cx.listener(Self::query_backspace))
                                .on_action(cx.listener(Self::query_delete_word_backward))
                                .on_action(cx.listener(Self::query_left))
                                .on_action(cx.listener(Self::query_right))
                                .on_action(cx.listener(Self::query_select_left))
                                .on_action(cx.listener(Self::query_select_right))
                                .on_action(cx.listener(Self::query_select_all))
                                .on_action(cx.listener(Self::query_home))
                                .on_action(cx.listener(Self::query_end))
                                .on_action(cx.listener(Self::query_show_character_palette))
                                .on_action(cx.listener(Self::query_paste))
                                .on_action(cx.listener(Self::query_cut))
                                .on_action(cx.listener(Self::query_copy))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, event, window, cx| {
                                        this.text_input_on_mouse_down(
                                            TextInputTarget::ParameterName,
                                            event,
                                            window,
                                            cx,
                                        );
                                    }),
                                )
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(|this, event, window, cx| {
                                        this.text_input_on_mouse_up(
                                            TextInputTarget::ParameterName,
                                            event,
                                            window,
                                            cx,
                                        );
                                    }),
                                )
                                .on_mouse_up_out(
                                    MouseButton::Left,
                                    cx.listener(|this, event, window, cx| {
                                        this.text_input_on_mouse_up(
                                            TextInputTarget::ParameterName,
                                            event,
                                            window,
                                            cx,
                                        );
                                    }),
                                )
                                .on_mouse_move(cx.listener(|this, event, window, cx| {
                                    this.text_input_on_mouse_move(
                                        TextInputTarget::ParameterName,
                                        event,
                                        window,
                                        cx,
                                    );
                                }))
                                .bg(scale_alpha(
                                    palette.selected_bg,
                                    if palette.dark { 0.95 } else { 0.75 },
                                ))
                                .border_1()
                                .border_color(palette.selected_border);
                        }

                        name_rows = name_rows.child(
                            div()
                                .id(("parameter-name-field", ix as u64))
                                .w_full()
                                .p_1()
                                .rounded_md()
                                .bg(if is_focus {
                                    scale_alpha(
                                        palette.selected_bg,
                                        if palette.dark { 0.75 } else { 0.45 },
                                    )
                                } else {
                                    scale_alpha(
                                        palette.row_hover_bg,
                                        if palette.dark { 0.92 } else { 1.0 },
                                    )
                                })
                                .border_1()
                                .border_color(if is_focus {
                                    palette.selected_border
                                } else {
                                    scale_alpha(
                                        palette.window_border,
                                        if palette.dark { 0.88 } else { 1.0 },
                                    )
                                })
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.focus_parameter_name_index(ix, cx);
                                }))
                                .child(
                                    div()
                                        .w_full()
                                        .text_xs()
                                        .text_color(palette.muted_text)
                                        .child(target.clone()),
                                )
                                .child(if is_focus {
                                    name_input.child(TextInputElement::new(
                                        cx.entity(),
                                        TextInputTarget::ParameterName,
                                        "name",
                                        palette,
                                        true,
                                    ))
                                } else {
                                    div()
                                        .w_full()
                                        .mt_1()
                                        .text_sm()
                                        .text_color(value_color)
                                        .child(value_display)
                                }),
                        );
                    }
                }

                content = content.child(
                    div()
                        .w_full()
                        .p_2()
                        .bg(scale_alpha(
                            palette.row_hover_bg,
                            if palette.dark { 0.95 } else { 1.0 },
                        ))
                        .border_1()
                        .border_color(palette.selected_border)
                        .rounded_lg()
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(palette.title_text)
                                        .child(format!("Parameter Name • Snippet #{item_id}")),
                                ),
                        )
                        .child(name_rows),
                );
            }
        }

        if let Some(item_id) = self.parameter_fill_target_id {
            let parameters = self
                .items
                .iter()
                .find(|entry| entry.id == item_id)
                .map(|entry| entry.parameters.clone())
                .unwrap_or_default();
            let parameter_fill_focus_handle =
                self.text_input_focus_handle(TextInputTarget::ParameterFill);
            let mut fill_rows = div().w_full().mt_1().flex().flex_col().gap_1();
            for (ix, parameter) in parameters.iter().enumerate() {
                let is_focus = ix == self.parameter_fill_focus_index;
                let value = self
                    .parameter_fill_values
                    .get(ix)
                    .cloned()
                    .unwrap_or_default();
                let value_display = if value.is_empty() {
                    "Type value…".to_owned()
                } else {
                    value
                };
                let value_color = if value_display == "Type value…" {
                    palette.query_placeholder
                } else {
                    palette.query_active
                };
                let mut fill_input = div()
                    .w_full()
                    .mt_1()
                    .px_1()
                    .rounded_sm()
                    .line_height(px(22.0))
                    .text_sm()
                    .font_weight(FontWeight::NORMAL);
                if is_focus {
                    fill_input = fill_input
                        .key_context("PastaTextInput")
                        .track_focus(&parameter_fill_focus_handle)
                        .cursor(CursorStyle::IBeam)
                        .on_action(cx.listener(Self::query_backspace))
                        .on_action(cx.listener(Self::query_delete_word_backward))
                        .on_action(cx.listener(Self::query_left))
                        .on_action(cx.listener(Self::query_right))
                        .on_action(cx.listener(Self::query_select_left))
                        .on_action(cx.listener(Self::query_select_right))
                        .on_action(cx.listener(Self::query_select_all))
                        .on_action(cx.listener(Self::query_home))
                        .on_action(cx.listener(Self::query_end))
                        .on_action(cx.listener(Self::query_show_character_palette))
                        .on_action(cx.listener(Self::query_paste))
                        .on_action(cx.listener(Self::query_cut))
                        .on_action(cx.listener(Self::query_copy))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event, window, cx| {
                                this.text_input_on_mouse_down(
                                    TextInputTarget::ParameterFill,
                                    event,
                                    window,
                                    cx,
                                );
                            }),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, event, window, cx| {
                                this.text_input_on_mouse_up(
                                    TextInputTarget::ParameterFill,
                                    event,
                                    window,
                                    cx,
                                );
                            }),
                        )
                        .on_mouse_up_out(
                            MouseButton::Left,
                            cx.listener(|this, event, window, cx| {
                                this.text_input_on_mouse_up(
                                    TextInputTarget::ParameterFill,
                                    event,
                                    window,
                                    cx,
                                );
                            }),
                        )
                        .on_mouse_move(cx.listener(|this, event, window, cx| {
                            this.text_input_on_mouse_move(
                                TextInputTarget::ParameterFill,
                                event,
                                window,
                                cx,
                            );
                        }))
                        .bg(scale_alpha(
                            palette.selected_bg,
                            if palette.dark { 0.95 } else { 0.75 },
                        ))
                        .border_1()
                        .border_color(palette.selected_border);
                }

                fill_rows = fill_rows.child(
                    div()
                        .id(("parameter-fill-field", ix as u64))
                        .w_full()
                        .p_1()
                        .rounded_md()
                        .bg(if is_focus {
                            scale_alpha(palette.selected_bg, if palette.dark { 0.78 } else { 0.48 })
                        } else {
                            scale_alpha(palette.row_hover_bg, if palette.dark { 0.92 } else { 1.0 })
                        })
                        .border_1()
                        .border_color(if is_focus {
                            palette.selected_border
                        } else {
                            scale_alpha(
                                palette.window_border,
                                if palette.dark { 0.88 } else { 1.0 },
                            )
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.focus_parameter_fill_index(ix, cx);
                        }))
                        .child(
                            div()
                                .w_full()
                                .text_xs()
                                .text_color(palette.muted_text)
                                .child(parameter.name.clone()),
                        )
                        .child(if is_focus {
                            fill_input.child(TextInputElement::new(
                                cx.entity(),
                                TextInputTarget::ParameterFill,
                                "Type value…",
                                palette,
                                true,
                            ))
                        } else {
                            div()
                                .w_full()
                                .mt_1()
                                .text_sm()
                                .text_color(value_color)
                                .child(value_display)
                        }),
                );
            }
            if parameters.is_empty() {
                fill_rows = fill_rows.child(
                    div()
                        .text_xs()
                        .text_color(palette.muted_text)
                        .child("No parameters found."),
                );
            }

            content = content.child(
                div()
                    .w_full()
                    .p_2()
                    .bg(scale_alpha(
                        palette.row_hover_bg,
                        if palette.dark { 0.95 } else { 1.0 },
                    ))
                    .border_1()
                    .border_color(palette.selected_border)
                    .rounded_lg()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(palette.title_text)
                                    .child(format!("Fill Parameters • Snippet #{item_id}")),
                            ),
                    )
                    .child(fill_rows)
                    .child(
                        div()
                            .w_full()
                            .mt_1()
                            .text_xs()
                            .text_color(palette.muted_text)
                            .child("blank all = original"),
                    ),
            );
        }

        if self.transform_menu_open {
            let mut transform_buttons = div()
                .w_full()
                .mt_1()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_start()
                .gap_1();
            for (ix, (label, action)) in [
                ("s  Shell quote", TransformAction::ShellQuote),
                ("j  JSON encode", TransformAction::JsonEncode),
                ("J  JSON decode", TransformAction::JsonDecode),
                ("f  JSON pretty", TransformAction::JsonPretty),
                ("F  JSON minify", TransformAction::JsonMinify),
                ("u  URL encode", TransformAction::UrlEncode),
                ("U  URL decode", TransformAction::UrlDecode),
                ("b  Base64 encode", TransformAction::Base64Encode),
                ("B  Base64 decode", TransformAction::Base64Decode),
                ("t  JWT decode", TransformAction::JwtDecode),
                ("e  Epoch decode", TransformAction::EpochDecode),
                ("h  SHA256 hash", TransformAction::Sha256Hash),
                ("c  Count stats", TransformAction::ContentStats),
                ("p  Cert info", TransformAction::PublicCertPemInfo),
                ("q  QR code", TransformAction::QrCode),
            ]
            .into_iter()
            .enumerate()
            {
                let button_bg =
                    scale_alpha(palette.row_hover_bg, if palette.dark { 0.95 } else { 1.0 });
                let button_border =
                    scale_alpha(palette.window_border, if palette.dark { 0.9 } else { 1.0 });
                let button_hover =
                    scale_alpha(palette.selected_bg, if palette.dark { 0.95 } else { 1.0 });
                transform_buttons = transform_buttons.child(
                    div()
                        .id(("transform-action", ix as u64))
                        .flex_none()
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .px(px(4.0))
                        .py(px(1.0))
                        .rounded_sm()
                        .bg(button_bg)
                        .border_1()
                        .border_color(button_border)
                        .text_size(px(10.0))
                        .line_height(px(14.0))
                        .text_color(palette.row_text)
                        .hover(move |style| style.bg(button_hover))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.apply_transform_action(action, cx);
                        }))
                        .child(label),
                );
            }

            content = content.child(
                div()
                    .w_full()
                    .p_2()
                    .bg(scale_alpha(
                        palette.row_hover_bg,
                        if palette.dark { 0.95 } else { 1.0 },
                    ))
                    .border_1()
                    .border_color(palette.selected_border)
                    .rounded_lg()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(palette.title_text)
                                    .child("Transforms"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(palette.muted_text)
                                    .child("Type shortcut or click"),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(palette.muted_text)
                            .child("Tab/Esc cancel"),
                    )
                    .child(transform_buttons),
            );
        }

        let workspace = if self.emoji_search_active {
            self.render_emoji_search_workspace(palette, window, cx)
        } else {
            div()
                .w_full()
                .flex_1()
                .min_h(px(0.0))
                .overflow_hidden()
                .flex()
                .gap_2()
                .child(
                    div()
                        .w(relative(RESULTS_LIST_WIDTH_RATIO))
                        .h_full()
                        .min_w(px(0.0))
                        .pt(px(4.0))
                        .pb(px(12.0))
                        .overflow_hidden()
                        .child(results),
                )
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .min_w(px(0.0))
                        .pt(px(4.0))
                        .pb(px(12.0))
                        .child(self.render_preview_pane(palette)),
                )
                .into_any_element()
        };

        content = content
            .child(div().w_full().h(px(1.0)).bg(palette.list_divider))
            .child(workspace);

        // The full command reference expands above the permanent action bar.
        if self.show_command_help {
            content = content.child(
                div()
                    .w_full()
                    .flex_none()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted_text)
                            .child("Commands"),
                    )
                    .child(render_help_run(&command_help_tips(), palette)),
            );
        }

        panel
            .child(content)
            .child(self.render_action_bar(palette, cx))
    }
}

impl LauncherView {
    /// Replaces the normal results+preview workspace while emoji search mode
    /// is active — a grid of glyph/name candidates from `emoji::search_emojis`.
    /// The search input itself is the main query row above (see the "Emoji"
    /// chip logic in `render`), not a separate field.
    fn render_emoji_search_workspace(
        &mut self,
        palette: Palette,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let result_count = self.emoji_search_results.len();
        let results = if result_count == 0 {
            div()
                .id("emoji-results-list")
                .w_full()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(palette.muted_text)
                .text_sm()
                .child("No emoji found.")
                .into_any_element()
        } else {
            let selected_index = self.emoji_search_selected_index;
            let row_count = result_count.div_ceil(EMOJI_GRID_COLUMNS);
            uniform_list(
                "emoji-results-list",
                row_count,
                cx.processor(move |this, range: Range<usize>, _window, cx| {
                    let mut rows = Vec::with_capacity(range.end.saturating_sub(range.start));
                    for row in range {
                        rows.push(this.render_emoji_grid_row(row, selected_index, palette, cx));
                    }
                    rows
                }),
            )
            .w_full()
            .h_full()
            .track_scroll(self.emoji_results_scroll.clone())
            .into_any_element()
        };

        div()
            .w_full()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h(px(0.0))
                    .pt(px(4.0))
                    .pb(px(12.0))
                    .overflow_hidden()
                    .child(results),
            )
            .into_any_element()
    }

    /// One virtualized `uniform_list` row of the emoji grid — up to
    /// `EMOJI_GRID_COLUMNS` tiles, each an equal relative share of the
    /// row's width so the tile count doesn't depend on measuring the
    /// panel's actual pixel width.
    fn render_emoji_grid_row(
        &self,
        row: usize,
        selected_index: usize,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let start = row * EMOJI_GRID_COLUMNS;
        let end = (start + EMOJI_GRID_COLUMNS).min(self.emoji_search_results.len());

        let mut tiles = div()
            .id(("emoji-row", row as u64))
            .w_full()
            .h(px(84.0))
            .flex()
            .items_center()
            .gap_x(px(6.0));
        for position in start..end {
            let Some(&entry_index) = self.emoji_search_results.get(position) else {
                continue;
            };
            tiles = tiles.child(self.render_emoji_tile(
                position,
                entry_index,
                selected_index,
                palette,
                cx,
            ));
        }
        // Pad a short trailing row with invisible spacers so the real tiles
        // keep the same `flex_1` share of the row width as a full row,
        // instead of stretching to fill the space the missing tiles left.
        for _ in (end - start)..EMOJI_GRID_COLUMNS {
            tiles = tiles.child(div().flex_1().min_w(px(0.0)).h(px(72.0)));
        }
        tiles.into_any_element()
    }

    fn render_emoji_tile(
        &self,
        position: usize,
        entry_index: usize,
        selected_index: usize,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((glyph, _name)) = emoji::entry_at(entry_index) else {
            return div().into_any_element();
        };
        let is_selected = position == selected_index;

        // Each tile fills its flex slot as a near-square chip carrying the
        // highlight/hover. Row `gap_x` handles horizontal spacing and the row
        // being taller than the tile handles vertical spacing — so both gaps
        // are small explicit values rather than whatever's left over from
        // floating a small square in a full-width column.
        let mut tile = div()
            .id(("emoji-tile", position as u64))
            .flex_1()
            .min_w(px(0.0))
            .h(px(72.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            // Reserve the 2px selection ring on every tile (transparent when
            // unselected) so moving the selection only recolors the border
            // rather than resizing the tile and shifting the grid.
            .border_2()
            .border_color(hsla(0.0, 0.0, 0.0, 0.0))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.emoji_search_selected_index = position;
                this.copy_selected_emoji(cx);
            }));
        if is_selected {
            tile = tile
                .border_color(palette.selected_border)
                .bg(palette.selected_bg);
        } else {
            tile = tile.hover({
                let row_hover = palette.row_hover_bg;
                move |style| style.bg(row_hover)
            });
        }

        tile.child(emoji_tile_glyph(glyph)).into_any_element()
    }

    /// The permanent bottom bar. It names the primary action for the current
    /// selection and an always-available Commands entry, both with keycaps, so
    /// the app reads as keyboard-first before a key is pressed.
    fn render_action_bar(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (primary_label, primary_key) = if self.emoji_search_active {
            ("Copy", "↵")
        } else {
            match self.items.get(self.selected_index) {
                Some(item)
                    if item.item_type == ClipboardItemType::Password
                        && self.is_secret_masked(item.id) =>
                {
                    (
                        "Reveal",
                        if cfg!(target_os = "macos") {
                            "⌘R"
                        } else {
                            "Ctrl+R"
                        },
                    )
                }
                _ => ("Copy", "↵"),
            }
        };
        let commands_key = if cfg!(target_os = "macos") {
            "⌘H"
        } else {
            "Ctrl+H"
        };
        let pin_key = if cfg!(target_os = "macos") {
            "⌘⇧P"
        } else {
            "Ctrl+Shift+P"
        };
        let pin_label = if self.pinned { "Unpin" } else { "Pin" };
        let status_label: SharedString = if self.emoji_search_active {
            self.emoji_search_results
                .get(self.emoji_search_selected_index)
                .copied()
                .and_then(emoji::entry_at)
                .map(|(_, name)| SharedString::from(name.to_owned()))
                .unwrap_or_else(|| SharedString::from("Emoji picker"))
        } else if self.pinned {
            SharedString::from("Pinned — won't auto-hide")
        } else {
            SharedString::from("Clipboard history")
        };

        div()
            .w_full()
            .flex_none()
            .h(px(40.0))
            .px_3()
            .flex()
            .items_center()
            .justify_between()
            .bg(palette.action_bar_bg)
            // GPUI's overflow_hidden clips children to a rectangular bounds mask,
            // not the panel's rounded corner shape, so this full-bleed background
            // needs its own matching radius or its flat corners show past the
            // panel's curve at the bottom edge.
            .rounded_bl(px(9.0))
            .rounded_br(px(9.0))
            .border_t_1()
            .border_color(palette.list_divider)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().size(px(6.0)).rounded_full().bg(palette.accent))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(palette.muted_text)
                            .child(status_label),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(palette.row_text)
                                    .child(primary_label),
                            )
                            .child(primary_keycap(primary_key, palette)),
                    )
                    .child(div().w(px(1.0)).h(px(16.0)).bg(palette.list_divider))
                    .child(
                        div()
                            .id("action-bar-pin")
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pinned = !this.pinned;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(if self.pinned {
                                        palette.accent
                                    } else {
                                        palette.row_text
                                    })
                                    .child(pin_label),
                            )
                            .child(keycap(pin_key, palette)),
                    )
                    .child(div().w(px(1.0)).h(px(16.0)).bg(palette.list_divider))
                    .child(
                        div()
                            .id("action-bar-commands")
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_command_help = !this.show_command_help;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(palette.row_text)
                                    .child("Commands"),
                            )
                            .child(keycap(commands_key, palette)),
                    ),
            )
    }

    fn render_tag_search_suggestions(
        &self,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut chips = div().w_full().flex().flex_row().flex_wrap().gap_1();
        for (ix, suggestion) in self.tag_search_suggestions.iter().enumerate() {
            let is_primary = ix == 0;
            let chip_bg = if is_primary {
                scale_alpha(palette.selected_bg, if palette.dark { 0.92 } else { 0.72 })
            } else {
                scale_alpha(palette.row_hover_bg, if palette.dark { 0.9 } else { 1.0 })
            };
            let chip_border = if is_primary {
                palette.selected_border
            } else {
                scale_alpha(palette.window_border, if palette.dark { 0.84 } else { 0.9 })
            };
            let chip_text = if is_primary {
                palette.title_text
            } else {
                palette.muted_text
            };

            chips = chips.child(
                div()
                    .id(("tag-search-suggestion", ix))
                    .text_xs()
                    .text_color(chip_text)
                    .bg(chip_bg)
                    .border_1()
                    .border_color(chip_border)
                    .rounded_md()
                    .px_1()
                    .py(px(1.0))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.apply_tag_search_suggestion_index(ix, cx);
                    }))
                    .child(search_suggestion_label(&self.query, suggestion)),
            );
        }

        div()
            .w_full()
            .mt_1()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .w_full()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted_text)
                            .child(search_suggestion_heading(&self.query)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted_text)
                            .child("↹ autocomplete"),
                    ),
            )
            .child(chips)
            .into_any_element()
    }

    fn render_preview_pane(&self, palette: Palette) -> AnyElement {
        let mut pane = div()
            .w_full()
            .h_full()
            .min_w(px(0.0))
            .py_2()
            .px(px(12.0))
            .bg(scale_alpha(
                palette.row_hover_bg,
                if palette.dark { 0.92 } else { 1.0 },
            ))
            .border_1()
            .border_color(scale_alpha(
                palette.window_border,
                if palette.dark { 0.9 } else { 1.0 },
            ))
            .rounded_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap_2();

        let Some(item) = self.items.get(self.selected_index) else {
            return pane
                .items_center()
                .justify_center()
                .text_center()
                .child(div().text_sm().text_color(palette.muted_text).child(
                    if self.query.is_empty() {
                        "Nothing to inspect."
                    } else {
                        "No matches."
                    },
                ))
                .into_any_element();
        };

        let Some(row_data) = self.row_presentations.get(self.selected_index) else {
            return pane.into_any_element();
        };

        let is_masked_secret =
            item.item_type == ClipboardItemType::Password && self.is_secret_masked(item.id);
        let preview_settled = Instant::now().duration_since(self.selection_changed_at)
            >= Duration::from_millis(PREVIEW_SETTLE_DELAY_MS);
        let qr_overlay_active = self
            .qr_preview
            .as_ref()
            .is_some_and(|(id, _)| *id == item.id);
        let preview_language = if qr_overlay_active || is_masked_secret {
            None
        } else {
            row_data.detected_language
        };
        let preview_text = if is_masked_secret {
            row_data.masked_preview.clone()
        } else if preview_settled {
            row_data.expanded_preview.clone()
        } else {
            row_data.collapsed_preview.clone()
        };
        let created_detail = format_timestamp_detail(&item.created_at);
        // Syntax highlighting is always on; Pasta no longer exposes a toggle for it.
        let preview_syntax_enabled = self.query.trim().is_empty()
            && !is_masked_secret
            && !qr_overlay_active
            && preview_settled
            && row_data.expanded_preview.len() <= PREVIEW_PANE_SYNTAX_MAX_CHARS
            && row_data.expanded_preview_line_count <= PREVIEW_PANE_SYNTAX_MAX_LINES;

        pane = pane.child(
            div()
                .w_full()
                .text_xs()
                .text_color(palette.row_meta_text)
                .child(created_detail),
        );

        if let Some(image) = &item.image {
            pane = pane.child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(palette.muted_text)
                    .child(format!(
                        "{} · {}",
                        image.mime_type,
                        format_image_metadata(image)
                    )),
            );
        }

        if !item.description.trim().is_empty() {
            pane = pane.child(
                div()
                    .w_full()
                    .p_2()
                    .bg(scale_alpha(
                        palette.selected_bg,
                        if palette.dark { 0.65 } else { 0.38 },
                    ))
                    .rounded_md()
                    .child(div().text_xs().text_color(palette.muted_text).child("Info"))
                    .child(
                        div()
                            .mt_1()
                            .text_sm()
                            .text_color(palette.row_text)
                            .child(item.description.clone()),
                    ),
            );
        }

        if !item.parameters.is_empty() {
            let mut parameter_row = div().w_full().mt_1().flex().flex_row().flex_wrap().gap_1();
            for parameter in item.parameters.iter().take(8) {
                parameter_row = parameter_row.child(
                    div()
                        .text_xs()
                        .text_color(palette.row_text)
                        .bg(scale_alpha(
                            palette.row_hover_bg,
                            if palette.dark { 0.95 } else { 1.0 },
                        ))
                        .border_1()
                        .border_color(scale_alpha(
                            palette.window_border,
                            if palette.dark { 0.9 } else { 1.0 },
                        ))
                        .rounded_md()
                        .px_1()
                        .child(parameter.name.clone()),
                );
            }

            pane = pane.child(
                div()
                    .w_full()
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted_text)
                            .child(format!("Parameters ({})", item.parameters.len())),
                    )
                    .child(parameter_row),
            );
        }

        if row_data.expanded_preview_truncated {
            pane = pane.child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(palette.muted_text)
                    .child("Preview shortened for speed."),
            );
        }

        if qr_overlay_active {
            pane = pane.child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(palette.muted_text)
                    .child("QR preview • Esc to dismiss"),
            );
        }

        pane = pane.child(div().w_full().h(px(1.0)).bg(palette.list_divider));

        if qr_overlay_active {
            let (modules, width) = match self.qr_preview.as_ref() {
                Some((_, matrix)) => (matrix.modules.clone(), matrix.width),
                None => (Vec::new(), 0),
            };
            pane.child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(qr_canvas_element(modules, width)),
            )
            .into_any_element()
        } else if let Some(image) = &item.image {
            pane.child(
                div().w_full().flex_1().min_h(px(0.0)).child(
                    img(image.path.clone())
                        .w_full()
                        .h_full()
                        .object_fit(ObjectFit::Contain),
                ),
            )
            .into_any_element()
        } else {
            pane.child(
                div()
                    .id(("preview-scroll", item.id as u64))
                    .w_full()
                    .flex_1()
                    .overflow_y_scroll()
                    .pr_2()
                    .child(
                        div()
                            .w_full()
                            .text_sm()
                            .text_color(palette.row_text)
                            .font_family(self.content_font_family.clone())
                            .child(syntax_styled_text(
                                &preview_text,
                                preview_language,
                                preview_syntax_enabled,
                                palette.dark,
                            )),
                    ),
            )
            .into_any_element()
        }
    }

    fn render_result_row(
        &self,
        ix: usize,
        item: &ClipboardRecord,
        row_data: &CachedRowPresentation,
        palette: Palette,
        info_editor_open: bool,
        tag_editor_open: bool,
        bowl_editor_open: bool,
        parameter_editor_open: bool,
        parameter_fill_open: bool,
        transform_menu_open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_selected = ix == self.selected_index;

        // The only inline chip a dense row keeps is secret state.
        let secret_pill = if item.item_type == ClipboardItemType::Password {
            Some(
                self.secret_seconds_left(item.id)
                    .map(|seconds| format!("OPEN {seconds}s"))
                    .unwrap_or_else(|| "LOCKED".to_owned()),
            )
        } else {
            None
        };

        let interactive = !info_editor_open
            && !tag_editor_open
            && !bowl_editor_open
            && !parameter_editor_open
            && !parameter_fill_open
            && !transform_menu_open;

        // Selection is fill only — a flat rounded pill, no border, flush with
        // the list edges with content inset by its own horizontal padding
        // (matching the preview pane's border-flush + p_2 pattern).
        let mut fill = div()
            .id(("result", item.id as u64))
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(8.0))
            .rounded_md()
            .overflow_hidden();
        if is_selected {
            fill = fill.bg(palette.selected_bg);
        }
        if interactive {
            fill = fill
                .hover({
                    let row_hover = palette.row_hover_bg;
                    move |style| style.bg(row_hover)
                })
                .cursor_pointer()
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    let is_double_click =
                        matches!(event, ClickEvent::Mouse(mouse) if mouse.up.click_count >= 2);
                    if is_double_click {
                        this.copy_index_to_clipboard(ix, cx);
                    } else {
                        this.select_result_index(ix, cx);
                    }
                }));
        }

        // Leading type icon — the one spot color lands on in the list.
        fill = fill.child(
            div()
                .flex_none()
                .w(px(15.0))
                .flex()
                .justify_center()
                .text_size(px(14.0))
                .text_color(type_color(item.item_type, palette.dark))
                .child(type_icon_glyph(item.item_type)),
        );

        // Title — single line, ellipsized on overflow. Image rows swap the
        // text title for a small thumbnail plus dimensions/size label.
        if let Some(image) = &item.image {
            fill = fill.child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        img(image.path.clone())
                            .flex_none()
                            .w(px(24.0))
                            .h(px(24.0))
                            .rounded(px(4.0))
                            .object_fit(ObjectFit::Cover),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .text_size(px(14.0))
                            .text_color(palette.row_text)
                            .child(row_data.title.clone()),
                    ),
            );
        } else {
            fill = fill.child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .text_size(px(14.0))
                    .text_color(palette.row_text)
                    .child(row_data.title.clone()),
            );
        }

        if let Some(pill) = secret_pill {
            fill = fill.child(
                div()
                    .flex_none()
                    .text_size(px(10.0))
                    .line_height(px(14.0))
                    .text_color(tag_chip_color(&pill, palette.dark))
                    .bg(palette.keycap_bg)
                    .rounded(px(4.0))
                    .px(px(6.0))
                    .py(px(1.0))
                    .whitespace_nowrap()
                    .child(pill),
            );
        }

        // Trailing timestamp.
        fill = fill.child(
            div()
                .flex_none()
                .text_size(px(11.0))
                .text_color(palette.row_meta_text)
                .child(row_data.created_label.clone()),
        );

        div()
            .w_full()
            .h(px(RESULT_ROW_HEIGHT))
            .py(px(2.0))
            .child(fill)
            .into_any_element()
    }
}

fn search_suggestion_heading(query: &str) -> &'static str {
    match parse_search_query(query) {
        SearchQuery::TagOnly { .. } => "Tag suggestions",
        SearchQuery::Bowl { .. } | SearchQuery::ExportBowl { .. } => "Bowl suggestions",
        SearchQuery::Default { .. } => "Suggestions",
    }
}

fn search_suggestion_label(query: &str, suggestion: &str) -> String {
    match parse_search_query(query) {
        SearchQuery::TagOnly { .. } => format!(":{suggestion}"),
        SearchQuery::Bowl { .. } => format!(":b {suggestion}"),
        SearchQuery::ExportBowl { .. } => format!(":e {suggestion}"),
        SearchQuery::Default { .. } => suggestion.to_owned(),
    }
}

/// A small key-hint pill, reused across the action bar and hints.
fn keycap(label: &str, palette: Palette) -> impl IntoElement {
    div()
        .flex_none()
        .text_size(px(11.0))
        .line_height(px(14.0))
        .text_color(palette.keycap_text)
        .bg(palette.keycap_bg)
        .rounded(px(4.0))
        .px(px(6.0))
        .py(px(2.0))
        .whitespace_nowrap()
        .child(label.to_owned())
}

/// The action bar's primary-action keycap — the one place a keycap gets the
/// brand accent instead of the neutral keycap fill, so the default action
/// reads as the obvious one to press.
fn primary_keycap(label: &str, palette: Palette) -> impl IntoElement {
    div()
        .flex_none()
        .text_size(px(11.0))
        .line_height(px(14.0))
        .text_color(palette.query_active)
        .bg(palette.accent)
        .rounded(px(4.0))
        .px(px(6.0))
        .py(px(2.0))
        .whitespace_nowrap()
        .child(label.to_owned())
}

fn command_help_tips() -> Vec<&'static str> {
    if cfg!(target_os = "macos") {
        vec![
            "⏎ copy",
            "⌘R reveal",
            "⌘J/K/L/; nav",
            "⌘I info",
            "⌘P param",
            "Tab transforms",
            "⌘T +tags",
            "⌘⇧T -tags",
            "⌘B bowl",
            "⌘⇧B -bowl",
            "⌥⌘B import",
            ":b search bowl",
            ":e export bowl",
            "↹ autocomplete",
            "⌘⇧S secret",
            "⌘D delete",
            "⌘⇧P pin",
            "Esc close",
            "⌘Q quit",
            "⌘H hide help",
        ]
    } else {
        vec![
            "⏎ copy",
            "Ctrl+R reveal",
            "Ctrl+J/K/L/; nav",
            "Ctrl+I info",
            "Ctrl+P param",
            "Tab transforms",
            "Ctrl+T +tags",
            "Ctrl+⇧T -tags",
            "Ctrl+B bowl",
            "Ctrl+⇧B -bowl",
            "Ctrl+Alt+B import",
            ":b search bowl",
            ":e export bowl",
            "↹ autocomplete",
            "Ctrl+⇧S secret",
            "Ctrl+D delete",
            "Ctrl+⇧P pin",
            "Esc close",
            "Ctrl+Q quit",
            "Ctrl+H hide help",
        ]
    }
}

fn render_help_run(tips: &[&str], palette: Palette) -> impl IntoElement {
    let help_chip_bg = scale_alpha(palette.row_hover_bg, if palette.dark { 0.9 } else { 1.0 });
    let help_chip_border =
        scale_alpha(palette.window_border, if palette.dark { 0.84 } else { 0.9 });

    let mut chips = div().flex().flex_row().flex_wrap().items_center().gap_1();
    for tip in tips {
        chips = chips.child(
            div()
                .flex_shrink_0()
                .text_xs()
                .line_height(px(14.0))
                .text_color(palette.muted_text)
                .bg(help_chip_bg)
                .border_1()
                .border_color(help_chip_border)
                .rounded_md()
                .px_1()
                .py(px(2.0))
                .child((*tip).to_owned()),
        );
    }

    div().flex_none().max_w_full().child(chips)
}

fn qr_canvas_element(modules: Vec<bool>, width: usize) -> AnyElement {
    if width == 0 || modules.len() != width * width {
        return div().into_any_element();
    }

    const QUIET_ZONE: usize = 4;
    let total = width + 2 * QUIET_ZONE;
    let dark = hsla(0.0, 0.0, 0.0, 1.0);
    let light = hsla(0.0, 0.0, 1.0, 1.0);

    canvas(
        |_bounds, _window, _cx| (),
        move |bounds, _prepaint, window, _cx| {
            let available_w = f32::from(bounds.size.width);
            let available_h = f32::from(bounds.size.height);
            let side = available_w.min(available_h);
            if side <= 0.0 {
                return;
            }
            // Snap cell size to whole pixels so modules line up without seams.
            let cell = (side / total as f32).floor().max(1.0);
            let qr_side = cell * total as f32;
            let origin_x = f32::from(bounds.origin.x) + (available_w - qr_side) / 2.0;
            let origin_y = f32::from(bounds.origin.y) + (available_h - qr_side) / 2.0;

            let bg = gpui::Bounds {
                origin: point(px(origin_x), px(origin_y)),
                size: size(px(qr_side), px(qr_side)),
            };
            window.paint_quad(fill(bg, light));

            for row in 0..width {
                for col in 0..width {
                    if !modules[row * width + col] {
                        continue;
                    }
                    let x = origin_x + (col + QUIET_ZONE) as f32 * cell;
                    let y = origin_y + (row + QUIET_ZONE) as f32 * cell;
                    let rect = gpui::Bounds {
                        origin: point(px(x), px(y)),
                        size: size(px(cell), px(cell)),
                    };
                    window.paint_quad(fill(rect, dark));
                }
            }
        },
    )
    .size_full()
    .into_any_element()
}
