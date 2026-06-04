//! macOS-style toolbar icon buttons (borderless ghost controls).

use gpui::{App, ElementId, SharedString};
use gpui_component::{
    IconName, Sizable as _, Size,
    button::{Button, ButtonVariants as _},
};

/// Borderless toolbar control matching gpui-component ghost buttons (Mail, Notes).
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .icon(icon)
        .ghost()
        .with_size(Size::Medium)
        .tooltip(tooltip)
        .on_click(on_click)
}
