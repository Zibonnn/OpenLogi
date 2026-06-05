//! Settings UI — embedded in the main window sidebar; the standalone window is
//! kept only as a focus target for ⌘, (navigates the main window).

use gpui::{
    App, Context, Entity, ParentElement as _, Render, Styled as _, Subscription, Window, px,
};
use gpui_component::{select::SelectState, setting::Settings};

use crate::nav::SidebarNav;
use crate::settings_pages::{
    self, LanguageOption, general_page, language_page, new_language_select, permissions_page,
};
use crate::theme;
use crate::windows::{self, AuxWindow, WindowRegistry};

/// Standalone settings window (legacy); prefer [`open`] which focuses the main window.
pub struct SettingsView {
    #[allow(dead_code, reason = "held to keep the appearance observer alive")]
    appearance_obs: Option<Subscription>,
    language_select: Entity<SelectState<Vec<LanguageOption>>>,
}

impl SettingsView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            appearance_obs: None,
            language_select: new_language_select(window, cx),
        }
    }
}

impl AuxWindow for SettingsView {
    fn set_appearance_obs(&mut self, sub: Subscription) {
        self.appearance_obs = Some(sub);
    }
}

/// Focus the main window and show the given settings section in the detail pane.
pub fn open(cx: &mut App) {
    open_section(SidebarNav::General, cx);
}

pub fn open_section(nav: SidebarNav, cx: &mut App) {
    if let Some(handle) = cx.default_global::<WindowRegistry>().main.clone() {
        if let Some(app) = cx.default_global::<WindowRegistry>().main_view.clone() {
            app.update(cx, |view, cx| {
                view.set_nav(nav, cx);
            });
        }
        let _ = handle.update(cx, |_, window, _| {
            window.activate_window();
            if let Some(section) = nav.settings_section() {
                window.set_window_title(&format!("OpenLogi — {}", section_title(section)));
            }
        });
        cx.activate(true);
        #[cfg(target_os = "macos")]
        crate::platform::tray::show_in_dock();
        return;
    }

    // No main window yet — fall back to the auxiliary settings window.
    windows::open_or_focus(
        |reg| &mut reg.settings,
        "Settings",
        gpui::Size::new(px(820.), px(520.)),
        SettingsView::new,
        cx,
    );
}

fn section_title(section: settings_pages::SettingsSection) -> String {
    match section {
        settings_pages::SettingsSection::General => tr!("General").to_string(),
        settings_pages::SettingsSection::Permissions => tr!("Permissions").to_string(),
        settings_pages::SettingsSection::Language => tr!("Language").to_string(),
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let pal = theme::palette(cx);

        gpui::div()
            .size_full()
            .bg(pal.window_bg)
            .text_color(pal.text_primary)
            .child(
                Settings::new("settings-window")
                    .sidebar_width(px(210.))
                    .page(general_page())
                    .page(permissions_page(pal))
                    .page(language_page(self.language_select.clone())),
            )
    }
}
