//! System-tray / status-item presence. macOS-only today, via `NSStatusItem`
//! (which lives in the menu bar) over raw Cocoa FFI — GPUI exposes no
//! status-bar API.
//!
//! `tray` is the cross-platform-neutral name: macOS has the menu-bar status
//! item, Windows the system tray / notification area, Linux the
//! StatusNotifierItem spec. Only macOS is implemented, so the module carries no
//! stub — every caller gates on `cfg(target_os = "macos")` instead.
//!
//! Menu clicks can't reach GPUI's `App`, so they post a [`TrayEvent`] on a
//! channel that a dedicated task in `main.rs` drains.

#[cfg(target_os = "macos")]
pub use macos::{
    TrayDeviceRow, TrayEvent, install, reconcile_dock_visibility, refresh_labels, request_refresh,
    set_device_rows, set_visible, uninstall,
};

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    };

    use cocoa::base::id;
    use objc::runtime::{Object, Sel};
    use objc::{sel, sel_impl};
    use openlogi_core::device::DeviceKind;
    use tokio::sync::mpsc;
    use tracing::warn;

    use super::super::status_item::{
        self, ActionCallback, ActionTarget, ActivationPolicy, Menu, MenuItem, StatusItem,
    };
    use crate::platform::tray_row::TrayRowView;

    /// A request raised by clicking a status-bar menu item, or by a live
    /// language switch asking the drain task to re-localize the whole menu.
    #[derive(Debug, Clone, Copy)]
    pub enum TrayEvent {
        Open,
        Quit,
        /// Re-title Open/Quit *and* the device line for the current locale.
        Refresh,
    }

    /// One connected device row shown in the tray menu.
    #[derive(Debug, Clone)]
    pub struct TrayDeviceRow {
        pub name: String,
        pub kind: DeviceKind,
        pub battery_percent: Option<u8>,
    }

    const TARGET_CLASS: &str = "OpenLogiMenuTarget";

    // Read by the Objective-C action callbacks, which can't capture state.
    static MENU_TX: OnceLock<mpsc::UnboundedSender<TrayEvent>> = OnceLock::new();

    /// Open/Quit item pointers, kept so a live locale switch can re-title them.
    /// Stored as opaque menu-item handles; only touched on the main thread.
    static MENU_REFS: OnceLock<MenuRefs> = OnceLock::new();

    /// How many device rows the tray menu can show at once.
    const MAX_DEVICE_ROWS: usize = 8;

    /// The device-status row views, written by [`set_device_rows`] — one per
    /// connected device, spare rows hidden. Only ever touched on the main thread.
    static DEVICE_ITEMS: OnceLock<Vec<MenuItem>> = OnceLock::new();

    /// Retained row views backing each device menu item.
    static DEVICE_VIEWS: OnceLock<Vec<TrayRowView>> = OnceLock::new();

    /// The `NSStatusItem` itself, so [`set_visible`] can show / hide the icon.
    static STATUS_ITEM: OnceLock<StatusItem> = OnceLock::new();

    /// Whether the status item is currently installed in `NSStatusBar`.
    static INSTALLED: AtomicBool = AtomicBool::new(false);

    struct MenuRefs {
        open: MenuItem,
        quit: MenuItem,
    }

    struct InstalledMenu {
        menu: Menu,
        refs: MenuRefs,
        device_items: Vec<MenuItem>,
        device_views: Vec<TrayRowView>,
    }

    /// Install the status item. Main thread only.
    pub fn install(tx: mpsc::UnboundedSender<TrayEvent>) {
        if INSTALLED.swap(true, Ordering::AcqRel) {
            return;
        }

        let _ = MENU_TX.set(tx);

        let status_item = StatusItem::new();
        let _ = STATUS_ITEM.set(status_item);
        let name = crate::platform::branding::display_name();
        status_item.set_symbol_icon("computermouse.fill", name, name);

        let installed_menu = build_menu();
        let _ = DEVICE_ITEMS.set(installed_menu.device_items);
        let _ = DEVICE_VIEWS.set(installed_menu.device_views);
        let _ = MENU_REFS.set(installed_menu.refs);
        status_item.set_menu(installed_menu.menu);
    }

    /// Remove the status item from the system status bar during app teardown.
    pub fn uninstall() {
        if !INSTALLED.swap(false, Ordering::AcqRel) {
            return;
        }
        let Some(item) = STATUS_ITEM.get() else {
            return;
        };
        item.remove_from_status_bar();
    }

    fn build_menu() -> InstalledMenu {
        let target = action_target();
        let menu = Menu::new();

        let idle = rust_i18n::t!("No devices connected");
        let mut device_items = Vec::with_capacity(MAX_DEVICE_ROWS);
        let mut device_views = Vec::with_capacity(MAX_DEVICE_ROWS);
        for i in 0..MAX_DEVICE_ROWS {
            let view = TrayRowView::new_device();
            if i == 0 {
                view.update_empty(&idle);
            }
            let item = MenuItem::disabled_with_view(view.raw());
            item.set_hidden(i != 0);
            menu.add_item(item);
            device_items.push(item);
            device_views.push(view);
        }

        menu.add_separator();

        let open_selector = sel!(openOpenLogi:);
        let quit_selector = sel!(quitOpenLogi:);
        let open_title = tray_open_label();
        let open_item = MenuItem::action(&open_title, open_selector, &target);
        menu.add_item(open_item);
        let quit_title = tray_quit_label();
        let quit_item = MenuItem::action(&quit_title, quit_selector, &target);
        menu.add_item(quit_item);

        InstalledMenu {
            menu,
            refs: MenuRefs {
                open: open_item,
                quit: quit_item,
            },
            device_items,
            device_views,
        }
    }

    fn action_target() -> ActionTarget {
        let open_selector = sel!(openOpenLogi:);
        let quit_selector = sel!(quitOpenLogi:);
        let target_methods = [
            (open_selector, open_action as ActionCallback),
            (quit_selector, quit_action as ActionCallback),
        ];
        ActionTarget::new(TARGET_CLASS, &target_methods)
    }

    /// Show the app in the Dock + menu bar — called when a window opens, so the
    /// app menu (⌘Q, Settings, …) is available while the window is up.
    pub fn show_in_dock() {
        status_item::set_activation_policy(ActivationPolicy::Regular);
    }

    /// Drop the app out of the Dock + menu bar, leaving only the status item.
    pub fn hide_from_dock() {
        status_item::set_activation_policy(ActivationPolicy::Accessory);
    }

    /// Keep Dock visibility in step with user settings and open windows.
    pub fn reconcile_dock_visibility(cx: &gpui::App) {
        let Some(state) = cx.try_global::<crate::state::AppState>() else {
            return;
        };
        let settings = state.app_settings();
        if cx.windows().is_empty() && settings.hide_from_dock && settings.show_in_menu_bar {
            hide_from_dock();
        } else {
            show_in_dock();
        }
    }

    /// Show or hide the status-item icon without tearing it down.
    pub fn set_visible(visible: bool) {
        let Some(item) = STATUS_ITEM.get() else {
            return;
        };
        item.set_visible(visible);
    }

    /// Update the device rows — one per connected device. Spare rows are hidden;
    /// an empty list shows the "No devices connected" placeholder.
    pub fn set_device_rows(rows: &[TrayDeviceRow]) {
        let (Some(items), Some(views)) = (DEVICE_ITEMS.get(), DEVICE_VIEWS.get()) else {
            return;
        };

        if rows.is_empty() {
            let idle = rust_i18n::t!("No devices connected");
            if let (Some(first), Some(first_view)) = (items.first(), views.first()) {
                first_view.update_empty(&idle);
                first.set_hidden(false);
            }
            for item in items.iter().skip(1) {
                item.set_hidden(true);
            }
            return;
        }

        for (i, (item, view)) in items.iter().zip(views.iter()).enumerate() {
            if let Some(row) = rows.get(i) {
                view.update_device(&row.name, row.kind, row.battery_percent);
                item.set_hidden(false);
            } else {
                item.set_hidden(true);
            }
        }
    }

    /// Re-title the Open/Quit items for the current locale.
    pub fn refresh_labels() {
        let Some(refs) = MENU_REFS.get() else {
            return;
        };
        refs.open.set_title(&tray_open_label());
        refs.quit.set_title(&tray_quit_label());
    }

    fn tray_open_label() -> String {
        format!("Open {}", crate::platform::branding::display_name())
    }

    fn tray_quit_label() -> String {
        format!("Quit {}", crate::platform::branding::display_name())
    }

    /// Ask the drain task to re-localize the whole menu after a live language switch.
    pub fn request_refresh() {
        post(TrayEvent::Refresh);
    }

    extern "C" fn open_action(_this: &Object, _cmd: Sel, _sender: id) {
        post(TrayEvent::Open);
    }

    extern "C" fn quit_action(_this: &Object, _cmd: Sel, _sender: id) {
        post(TrayEvent::Quit);
    }

    fn post(event: TrayEvent) {
        if let Some(tx) = MENU_TX.get()
            && tx.send(event).is_err()
        {
            warn!(?event, "menu-bar event dropped — GPUI loop gone");
        }
    }
}
