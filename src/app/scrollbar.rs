use std::cell::Cell;
use std::rc::Rc;

use crate::*;
use gpui::{DispatchPhase, HitboxBehavior, ScrollWheelEvent, canvas};

/// Width of the column the scrollbar occupies beside its list.
pub(crate) const SCROLLBAR_WIDTH: f32 = 10.0;
const THUMB_WIDTH: f32 = 4.0;
const THUMB_HOVER_WIDTH: f32 = 6.0;
/// Keeps the thumb grabbable when the content is many viewports tall.
const THUMB_MIN_HEIGHT: f32 = 24.0;

/// Where the pointer grabbed the thumb, as a distance from the thumb's top,
/// while a drag is in progress. It lives in the view because a drag spans
/// many frames and the scrollbar element is rebuilt on every one.
#[derive(Clone, Default)]
pub(crate) struct ScrollbarDrag(Rc<Cell<Option<f32>>>);

/// Thumb placement within a track, in pixels from the track's top.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ThumbGeometry {
    top: f32,
    height: f32,
    /// How far the content can scroll; zero means nothing to scroll.
    max_scroll: f32,
}

impl ThumbGeometry {
    /// `None` when the content fits in the viewport.
    fn new(track: f32, viewport: f32, content: f32, scroll_top: f32) -> Option<Self> {
        let max_scroll = content - viewport;
        if track <= 0.0 || viewport <= 0.0 || max_scroll <= 0.5 {
            return None;
        }
        let height = (track * viewport / content).clamp(THUMB_MIN_HEIGHT.min(track), track);
        let travel = track - height;
        let top = travel * (scroll_top / max_scroll).clamp(0.0, 1.0);
        Some(Self {
            top,
            height,
            max_scroll,
        })
    }

    /// The scroll position that puts the thumb's top at `thumb_top`.
    fn scroll_top_for(&self, track: f32, thumb_top: f32) -> f32 {
        let travel = track - self.height;
        if travel <= 0.0 {
            return 0.0;
        }
        (thumb_top / travel).clamp(0.0, 1.0) * self.max_scroll
    }
}

/// A vertical scrollbar for the `uniform_list` tracking `handle`, meant to sit
/// in its own `SCROLLBAR_WIDTH` column to the right of the list. Drag the
/// thumb to scroll, click the track to jump there, or use the wheel over it.
/// It paints nothing while the list fits without scrolling.
pub(crate) fn vertical_scrollbar(
    handle: UniformListScrollHandle,
    drag: ScrollbarDrag,
    palette: Palette,
) -> impl IntoElement {
    canvas(
        {
            let handle = handle.clone();
            move |bounds, window, _cx| {
                let geometry = list_geometry(&handle, bounds);
                let hitbox = geometry.map(|_| window.insert_hitbox(bounds, HitboxBehavior::Normal));
                (geometry, hitbox)
            }
        },
        move |bounds, (geometry, hitbox), window, _cx| {
            let (Some(geometry), Some(hitbox)) = (geometry, hitbox) else {
                return;
            };
            let track = f32::from(bounds.size.height);
            let dragging = drag.0.get().is_some();
            let emphasized = dragging || hitbox.is_hovered(window);
            let thumb_width = if emphasized {
                THUMB_HOVER_WIDTH
            } else {
                THUMB_WIDTH
            };
            let thumb_color = if emphasized {
                palette.muted_text
            } else {
                palette.query_placeholder
            };
            let thumb = Bounds {
                origin: point(
                    bounds.origin.x + px((SCROLLBAR_WIDTH - thumb_width) / 2.0),
                    bounds.origin.y + px(geometry.top),
                ),
                size: size(px(thumb_width), px(geometry.height)),
            };
            window.paint_quad(fill(thumb, thumb_color).corner_radii(px(thumb_width / 2.0)));
            window.set_cursor_style(CursorStyle::Arrow, &hitbox);

            let set_scroll_top = {
                let handle = handle.clone();
                move |scroll_top: f32| {
                    let base = handle.0.borrow().base_handle.clone();
                    let x = base.offset().x;
                    base.set_offset(point(x, px(-scroll_top)));
                }
            };

            window.on_mouse_event({
                let drag = drag.clone();
                let hitbox = hitbox.clone();
                let set_scroll_top = set_scroll_top.clone();
                move |event: &MouseDownEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble
                        || event.button != MouseButton::Left
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    let pointer = f32::from(event.position.y - bounds.origin.y);
                    let on_thumb =
                        pointer >= geometry.top && pointer <= geometry.top + geometry.height;
                    let grab = if on_thumb {
                        pointer - geometry.top
                    } else {
                        let grab = geometry.height / 2.0;
                        set_scroll_top(geometry.scroll_top_for(track, pointer - grab));
                        grab
                    };
                    drag.0.set(Some(grab));
                    cx.stop_propagation();
                    window.refresh();
                }
            });

            window.on_mouse_event({
                let drag = drag.clone();
                let set_scroll_top = set_scroll_top.clone();
                move |event: &MouseMoveEvent, phase, window, _cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let Some(grab) = drag.0.get() else {
                        return;
                    };
                    if event.pressed_button != Some(MouseButton::Left) {
                        drag.0.set(None);
                        window.refresh();
                        return;
                    }
                    let pointer = f32::from(event.position.y - bounds.origin.y);
                    set_scroll_top(geometry.scroll_top_for(track, pointer - grab));
                    window.refresh();
                }
            });

            window.on_mouse_event({
                let drag = drag.clone();
                move |event: &MouseUpEvent, phase, window, _cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && drag.0.take().is_some()
                    {
                        window.refresh();
                    }
                }
            });

            // The column sits outside the list's own hitbox, so the list never
            // sees wheel events over it; apply them here instead.
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, _cx| {
                if phase != DispatchPhase::Bubble || !hitbox.should_handle_scroll(window) {
                    return;
                }
                let delta = f32::from(event.delta.pixel_delta(window.line_height()).y);
                let scroll_top = -f32::from(handle.0.borrow().base_handle.offset().y);
                set_scroll_top((scroll_top - delta).clamp(0.0, geometry.max_scroll));
                window.refresh();
            });
        },
    )
    .w(px(SCROLLBAR_WIDTH))
    .h_full()
}

/// Lays `list` out with a `vertical_scrollbar` in a column to its right. The
/// list should be `size_full` so it fills the space the scrollbar leaves.
pub(crate) fn with_vertical_scrollbar(
    list: impl IntoElement,
    handle: UniformListScrollHandle,
    drag: ScrollbarDrag,
    palette: Palette,
) -> gpui::Div {
    // The scrollbar reads the list's layout, so it must follow the list.
    div()
        .size_full()
        .flex()
        .flex_row()
        .child(div().flex_1().min_w(px(0.0)).h_full().flex().child(list))
        .child(vertical_scrollbar(handle, drag, palette))
}

/// Reads the list's viewport and content heights as captured by its most
/// recent layout. The list must come before the scrollbar in the element tree
/// so that layout has already happened this frame.
fn list_geometry(
    handle: &UniformListScrollHandle,
    bounds: Bounds<Pixels>,
) -> Option<ThumbGeometry> {
    let state = handle.0.borrow();
    let sizes = state.last_item_size?;
    ThumbGeometry::new(
        f32::from(bounds.size.height),
        f32::from(sizes.item.height),
        f32::from(sizes.contents.height),
        -f32::from(state.base_handle.offset().y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_thumb_when_content_fits() {
        assert_eq!(ThumbGeometry::new(300.0, 300.0, 300.0, 0.0), None);
        assert_eq!(ThumbGeometry::new(300.0, 300.0, 120.0, 0.0), None);
    }

    #[test]
    fn thumb_is_proportional_and_tracks_scroll_position() {
        let top = ThumbGeometry::new(300.0, 300.0, 600.0, 0.0).unwrap();
        assert_eq!((top.top, top.height), (0.0, 150.0));

        let bottom = ThumbGeometry::new(300.0, 300.0, 600.0, 300.0).unwrap();
        assert_eq!(bottom.top, 150.0);
    }

    #[test]
    fn thumb_keeps_a_minimum_height_for_long_content() {
        let geometry = ThumbGeometry::new(300.0, 300.0, 100_000.0, 0.0).unwrap();
        assert_eq!(geometry.height, THUMB_MIN_HEIGHT);
    }

    #[test]
    fn dragging_the_thumb_maps_back_to_a_clamped_scroll_position() {
        let geometry = ThumbGeometry::new(300.0, 300.0, 600.0, 0.0).unwrap();
        assert_eq!(geometry.scroll_top_for(300.0, 75.0), 150.0);
        assert_eq!(geometry.scroll_top_for(300.0, -40.0), 0.0);
        assert_eq!(geometry.scroll_top_for(300.0, 900.0), 300.0);
    }
}
