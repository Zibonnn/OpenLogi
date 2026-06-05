//! Popover content for binding mouse buttons, plus the gesture button's
//! cascading menu.
//!
//! - [`action_picker`] — one button → one [`Action`], rendered as a custom flat
//!   list inside a gpui-component [`Popover`](gpui_component::popover::Popover).
//!   Generic over the entity that should be notified after a binding changes so
//!   the trigger re-renders with the new label.
//! - [`build_gesture_menu`] — the gesture button's two-level
//!   [`PopupMenu`](gpui_component::menu::PopupMenu): one submenu per
//!   [`GestureDirection`], each listing the full action catalog with the
//!   current binding checked. Picking an action commits straight to
//!   [`AppState`], whose global observers re-render the model, so the menu
//!   needs no observer.
//!
//! The [`Popover`] wraps the [`action_picker`] content in a styled surface
//! (background, border, shadow, `p_3` padding), so the layout here stays flat:
//! no extra card background, no extra outer padding. Rows are transparent until
//! hovered; the active binding is marked with accent text plus a check glyph
//! rather than a filled box. The [`PopupMenu`] draws its own surface and check
//! marks, so the gesture menu defers entirely to the framework's styling.

use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, BorrowAppContext as _, Context, Entity, Focusable as _,
    FontWeight, InteractiveElement, IntoElement, ParentElement, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, hsla,
    point, prelude::FluentBuilder as _, px, rgb,
};
use gpui_component::{
    Icon, IconName, Sizable as _,
    h_flex,
    input::{Input, InputState},
    menu::{PopupMenu, PopupMenuItem},
    popover::PopoverState,
    v_flex,
};

use crate::data::mouse_buttons::{
    Action, ButtonId, Category, GestureDirection, default_gesture_binding,
};
use crate::state::AppState;
use crate::theme::{self, ACCENT_BLUE, Palette};

/// Width of the action picker popover.
const POPOVER_W: f32 = 288.;

/// Cap the scrollable action list height.
const POPOVER_LIST_MAX_H: f32 = 340.;

/// Commit callback invoked when a row is clicked.
type PickFn = Rc<dyn Fn(Action, &mut Window, &mut App)>;

// ── Stateful picker view ────────────────────────────────────────────────────

struct ActionPickerView {
    btn: ButtonId,
    current: Option<Action>,
    on_pick: PickFn,
    search: Entity<InputState>,
    scroll: ScrollHandle,
    last_query: String,
    #[allow(dead_code, reason = "held to keep the InputState observation alive")]
    search_sub: Subscription,
    focused: bool,
}

impl ActionPickerView {
    fn new(
        btn: ButtonId,
        current: Option<Action>,
        on_pick: PickFn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr!("Search actions…")));
        let search_sub = cx.observe(&search, |_, _entity, cx| cx.notify());
        Self {
            btn,
            current,
            on_pick,
            search,
            search_sub,
            scroll: ScrollHandle::new(),
            last_query: String::new(),
            focused: false,
        }
    }
}

impl Render for ActionPickerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Auto-focus the search input on first render so typing works immediately.
        if !self.focused {
            self.focused = true;
            self.search.read(cx).focus_handle(cx).focus(window, cx);
        }

        let pal = theme::palette(cx);
        let query_raw = self.search.read(cx).value();
        let query = query_raw.trim().to_lowercase();

        // Reset scroll to top when the search query changes.
        if query != self.last_query {
            self.last_query = query.clone();
            self.scroll.set_offset(point(px(0.), px(0.)));
        }

        let button_name = rust_i18n::t!(self.btn.label());

        let rows = filtered_action_rows(
            "action-item",
            &query,
            self.current.as_ref(),
            &self.on_pick,
            pal,
        );

        let list_content: AnyElement = if rows.is_empty() {
            div()
                .px_3()
                .py_4()
                .text_sm()
                .text_color(pal.text_muted)
                .child(tr!("No results"))
                .into_any_element()
        } else {
            div().children(rows).into_any_element()
        };

        v_flex()
            .w(px(POPOVER_W))
            .gap_1()
            .child(picker_title(tr!("Bind %{name}", name => button_name), pal))
            .child(search_bar(&self.search, pal))
            .child(divider(pal))
            .child(
                div()
                    .id("picker-scroll")
                    .max_h(px(POPOVER_LIST_MAX_H))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(list_content),
            )
    }
}

// ── Cached picker state ─────────────────────────────────────────────────────

/// Holds the `ActionPickerView` entity across re-renders of the popover content
/// closure. Without this cache, every render would construct a fresh `InputState`
/// (losing typed text) and a fresh `ScrollHandle` (losing scroll position).
/// Mirrors the pattern used by `GestureMenuState` for the gesture menu.
struct PickerCache {
    view: Entity<ActionPickerView>,
}

// ── Public entry points ─────────────────────────────────────────────────────

/// Build the popover body that re-binds a single `btn`.
///
/// `observer` is whatever entity wraps the trigger — it's notified after the
/// global updates so the trigger re-renders. Picking an action commits it and
/// dismisses the popover.
pub fn action_picker<T: 'static>(
    btn: ButtonId,
    observer: &Entity<T>,
    window: &mut Window,
    cx: &mut Context<PopoverState>,
) -> AnyElement {
    let current = cx
        .try_global::<AppState>()
        .and_then(|s| s.button_bindings.get(&btn).cloned());

    let observer = observer.clone();
    let popover_weak = cx.entity().downgrade();

    // Cache the view entity across re-renders so InputState and ScrollHandle
    // survive — see PickerCache doc comment.
    let cache = window.use_keyed_state("picker-view", cx, {
        let observer = observer.clone();
        let popover_weak = popover_weak.clone();
        move |window, cx| {
            let on_pick: PickFn = Rc::new(move |action, window, cx| {
                cx.update_global::<AppState, _>(|state, _| state.commit_binding(btn, action));
                observer.update(cx, |_, cx| cx.notify());
                if let Some(p) = popover_weak.upgrade() {
                    p.update(cx, |s, cx| s.dismiss(window, cx));
                }
            });
            let view =
                cx.new(|cx| ActionPickerView::new(btn, current, on_pick, window, cx));
            PickerCache { view }
        }
    });

    cache.read(cx).view.clone().into_any_element()
}

/// Build the gesture button's two-level [`PopupMenu`].
pub fn build_gesture_menu(
    mut menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    for dir in GestureDirection::ALL {
        let label = format!("{}  {}", dir.glyph(), tr!(dir.label()));
        menu = menu.submenu(label, window, cx, move |submenu, _window, cx| {
            gesture_action_submenu(dir, submenu, cx)
        });
    }
    menu
}

// ── Private helpers ─────────────────────────────────────────────────────────

fn gesture_action_submenu(
    direction: GestureDirection,
    submenu: PopupMenu,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let current = cx
        .try_global::<AppState>()
        .and_then(|s| s.gesture_bindings.get(&direction).cloned())
        .unwrap_or_else(|| default_gesture_binding(direction));

    let mut submenu = submenu.scrollable(true).max_h(px(POPOVER_LIST_MAX_H));
    for (category, actions) in grouped_catalog() {
        submenu = submenu.label(tr!(category.label()));
        for action in actions {
            let checked = action == current;
            let label = tr!(action.label());
            let commit = action;
            submenu = submenu.item(PopupMenuItem::new(label).checked(checked).on_click(
                move |_event, _window, cx| {
                    let action = commit.clone();
                    cx.update_global::<AppState, _>(move |state, _| {
                        state.commit_gesture_binding(direction, action);
                    });
                },
            ));
        }
    }
    submenu
}

/// The action catalog grouped by [`Category`], preserving catalog order.
fn grouped_catalog() -> Vec<(Category, Vec<Action>)> {
    let mut sections: Vec<(Category, Vec<Action>)> = Vec::new();
    for action in Action::catalog() {
        let cat = action.category();
        if let Some(sec) = sections.iter_mut().find(|(c, _)| *c == cat) {
            sec.1.push(action);
        } else {
            sections.push((cat, vec![action]));
        }
    }
    sections
}

/// Map each category to a representative icon.
fn category_icon(cat: Category) -> IconName {
    match cat {
        Category::Editing    => IconName::Replace,
        Category::Browser    => IconName::Globe,
        Category::Media      => IconName::Play,
        Category::Mouse      => IconName::ChevronsUpDown,
        Category::Dpi        => IconName::SortAscending,
        Category::Scroll     => IconName::ArrowDown,
        Category::Navigation => IconName::LayoutDashboard,
        Category::System     => IconName::SquareTerminal,
    }
}

/// Category-grouped, optionally filtered action rows.
fn filtered_action_rows(
    id_prefix: &'static str,
    query: &str,
    current: Option<&Action>,
    on_pick: &PickFn,
    pal: Palette,
) -> Vec<AnyElement> {
    let mut idx = 0usize;
    let mut children: Vec<AnyElement> = Vec::new();
    let is_first_ref = &mut true;

    for (category, actions) in grouped_catalog() {
        let matching: Vec<Action> = actions
            .into_iter()
            .filter(|action| {
                if query.is_empty() {
                    return true;
                }
                let label = rust_i18n::t!(action.label()).to_lowercase();
                let cat_label = rust_i18n::t!(category.label()).to_lowercase();
                label.contains(query) || cat_label.contains(query)
            })
            .collect();

        if matching.is_empty() {
            continue;
        }

        children.push(section_header(category, *is_first_ref, pal));
        *is_first_ref = false;

        for action in matching {
            let selected = current == Some(&action);
            let label = tr!(action.label());
            let on_pick = on_pick.clone();
            let row_id = idx;
            idx += 1;
            children.push(
                menu_row((id_prefix, row_id), pal)
                    .text_color(if selected {
                        rgb(ACCENT_BLUE).into()
                    } else {
                        pal.text_primary
                    })
                    .when(selected, |s| s.bg(hsla(0.6, 0.9, 0.6, 0.12)))
                    .child(div().flex_1().text_sm().child(label))
                    .when(selected, |s| {
                        s.child(
                            Icon::new(IconName::Check)
                                .size_3()
                                .text_color(rgb(ACCENT_BLUE)),
                        )
                    })
                    .on_click(move |_event, window, cx| (on_pick)(action.clone(), window, cx))
                    .into_any_element(),
            );
        }
    }
    children
}

/// A clickable, full-width menu row — indented under its category header.
fn menu_row(id: impl Into<gpui::ElementId>, pal: Palette) -> gpui::Stateful<gpui::Div> {
    h_flex()
        .id(id)
        .w_full()
        .items_center()
        .justify_between()
        .gap_2()
        .pl_6()
        .pr_3()
        .py_1p5()
        .rounded_md()
        .hover(move |s| s.bg(pal.surface_hover))
}

/// Section header with category icon and label.
fn section_header(category: Category, is_first: bool, pal: Palette) -> AnyElement {
    h_flex()
        .w_full()
        .px_3()
        .when(!is_first, |s| s.mt_2())
        .pt_2()
        .pb_1()
        .gap_1p5()
        .items_center()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(pal.text_muted)
        .child(
            Icon::new(category_icon(category))
                .size_3()
                .text_color(pal.text_muted),
        )
        .child(div().child(tr!(category.label())))
        .into_any_element()
}

/// Search field styled as a native macOS search bar — subtle filled background,
/// no hard border, `appearance(false)` lets our wrapper own the visual chrome.
fn search_bar(state: &Entity<InputState>, pal: Palette) -> impl IntoElement {
    let is_light = pal.card_bg.l > 0.5;
    let search_bg = if is_light {
        hsla(0., 0., 0., 0.07)
    } else {
        hsla(0., 0., 1., 0.10)
    };
    div()
        .mx_2()
        .my_0p5()
        .rounded_md()
        .bg(search_bg)
        .child(
            Input::new(state)
                .small()
                .appearance(false)
                .prefix(
                    Icon::new(IconName::Search)
                        .size_3()
                        .text_color(pal.text_muted)
                        .into_any_element(),
                ),
        )
}

/// Popover title line — "Bind Back", etc.
fn picker_title(text: impl Into<gpui::SharedString>, pal: Palette) -> impl IntoElement {
    div()
        .px_3()
        .pt_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(pal.text_muted)
        .child(text.into())
}

/// 1px hairline separating title/search from the list.
fn divider(pal: Palette) -> impl IntoElement {
    div().h(px(1.)).w_full().bg(pal.border)
}
