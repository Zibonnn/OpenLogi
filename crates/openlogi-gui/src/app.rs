use gpui::{
    AnyElement, AppContext as _, BorrowAppContext as _, Context, Div, Entity, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, WindowControlArea,
    div, img, prelude::FluentBuilder as _, px, rgb,
};
use gpui_component::{
    Collapsible, Icon, IconName,
    button::{Button, ButtonVariants as _},
    description_list::{DescriptionItem, DescriptionList},
    h_flex,
    scroll::ScrollableElement as _,
    select::SelectState,
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarItem, SidebarMenu, SidebarMenuItem,
    },
    tab::TabBar,
    v_flex,
};
use openlogi_core::config::Config;
use openlogi_core::device::{
    BatteryInfo, BatteryLevel, BatteryStatus, DeviceInventory, DeviceKind,
};
use openlogi_hid::DeviceRoute;
use openlogi_hook::Hook;
use tracing::{info, warn};

use crate::app_menu::{Minimize, Zoom};
use crate::asset::AssetResolver;
use crate::components::dpi_panel::DpiPanel;
use crate::components::lighting_panel::LightingPanel;
use crate::mouse_model::view::MouseModelView;
use crate::nav::SidebarNav;
use crate::settings_pages::{self, LanguageOption, embedded_settings_content, on_language_select};
use crate::state::{AppState, DeviceRecord};
use crate::theme::{self, FOOTER_H, Palette, card_shadow, card_shadow_hover};
use crate::windows::settings;

/// The active section of the device-detail screen. Backs the detail `TabBar`;
/// reset to the device's first tab whenever a device is opened.
///
/// The tab *set* depends on the device kind — see [`DetailTab::tabs_for`]. A
/// mouse gets button-mapping + pointer tuning; a wired keyboard gets RGB
/// lighting; every device gets the info tab. Tailoring the tabs is what keeps a
/// keyboard from rendering a mouse silhouette and an irrelevant DPI panel
/// (issue #19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    /// The mouse model with clickable button hotspots.
    Buttons,
    /// Pointer tuning — DPI and presets.
    Pointer,
    /// RGB lighting — color, brightness, on/off.
    Lighting,
    /// Device info and configuration (not named `Device` — collides with i18n).
    Info,
}

impl DetailTab {
    /// The detail sections shown for `record`, in tab order. Always non-empty:
    /// every device gets at least the info tab.
    fn tabs_for(record: &DeviceRecord) -> Vec<Self> {
        let mut tabs = Vec::new();
        if is_configurable_pointer(record.kind) {
            tabs.push(Self::Buttons);
            tabs.push(Self::Pointer);
        }
        if supports_lighting(record) {
            tabs.push(Self::Lighting);
        }
        tabs.push(Self::Info);
        tabs
    }

    /// The first (default) tab for `record` — what a freshly opened device shows.
    fn default_for(record: &DeviceRecord) -> Self {
        Self::tabs_for(record)
            .first()
            .copied()
            .unwrap_or(Self::Info)
    }

    fn label(self) -> SharedString {
        match self {
            Self::Buttons => tr!("Buttons"),
            Self::Pointer => tr!("Pointer"),
            Self::Lighting => tr!("Lighting"),
            Self::Info => tr!("Info"),
        }
    }
}

/// Whether a device drives the mouse model + DPI panel. Other kinds (keyboards,
/// numpads, headsets…) don't get a mouse silhouette that doesn't describe them
/// (issue #19); they fall back to the info tab — and, for wired keyboards, the
/// lighting tab.
fn is_configurable_pointer(kind: DeviceKind) -> bool {
    matches!(kind, DeviceKind::Mouse | DeviceKind::Trackball | DeviceKind::Unknown)
}

/// Whether to offer the RGB lighting tab — keyboards with per-key RGB only.
///
/// Do not key off `Unknown` + direct USB/BT: most mice report that pairing and
/// would incorrectly get a Lighting tab. Wired G-series boards that enumerate as
/// `Unknown` are rare; they can be whitelisted by codename when needed.
fn supports_lighting(record: &DeviceRecord) -> bool {
    matches!(record.kind, DeviceKind::Keyboard)
}

/// Root application view — System Settings layout: device sidebar + detail pane.
pub struct AppView {
    mouse_model: Entity<MouseModelView>,
    dpi_panel: Entity<DpiPanel>,
    lighting_panel: Entity<LightingPanel>,
    #[allow(dead_code, reason = "held to keep the appearance observer alive")]
    appearance_obs: Option<Subscription>,
    /// Re-renders the root when the device list changes so the empty state
    /// swaps to the device UI (and back) on hot-plug, without a restart.
    #[allow(dead_code, reason = "held to keep the AppState observer alive")]
    state_obs: Subscription,
    accessibility_dismissed: bool,
    /// Which section of the device-detail screen is showing.
    active_tab: DetailTab,
    /// Main-window sidebar selection (device or a settings section).
    nav: SidebarNav,
    language_select: Entity<SelectState<Vec<LanguageOption>>>,
    #[allow(
        dead_code,
        reason = "held to keep the language-select subscription alive"
    )]
    language_sub: Subscription,
}

impl AppView {
    /// Construct the root view and its child entities.
    pub fn new(
        inventories: &[DeviceInventory],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let config = match Config::load_or_default() {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "could not load config.toml — starting with defaults");
                Config::default()
            }
        };

        let cache = AssetResolver::new();

        if !cx.has_global::<AppState>() {
            cx.set_global(AppState::with_runtime(config, inventories, &cache));
        }

        if let Some(state) = cx.try_global::<AppState>() {
            if let Some(record) = state.current_record() {
                info!(
                    device_key = %record.config_key,
                    display = %record.display_name,
                    "initial device selected"
                );
            } else {
                info!(
                    root = ?cache.cache_root(),
                    "no devices with HID++ model info — using synthetic silhouette"
                );
            }
        }

        let mouse_model = cx.new(MouseModelView::new);
        let dpi_panel = cx.new(DpiPanel::new);
        let lighting_panel = cx.new(LightingPanel::new);
        let language_select = settings_pages::new_language_select(window, cx);
        let language_sub =
            cx.subscribe_in(&language_select, window, |_, select, event, window, cx| {
                on_language_select(select, event, window, cx);
            });
        let state_obs = cx.observe_global::<AppState>(|_, cx| cx.notify());
        Self {
            mouse_model,
            dpi_panel,
            lighting_panel,
            appearance_obs: None,
            state_obs,
            accessibility_dismissed: false,
            active_tab: DetailTab::Buttons,
            nav: SidebarNav::Devices,
            language_select,
            language_sub,
        }
    }

    /// Switch sidebar navigation (device detail or embedded settings).
    pub fn set_nav(&mut self, nav: SidebarNav, cx: &mut Context<Self>) {
        self.nav = nav;
        if let SidebarNav::Device(idx) = nav {
            cx.update_global::<AppState, _>(|state, _| state.set_current_device(idx));
            self.active_tab = cx
                .try_global::<AppState>()
                .and_then(AppState::current_record)
                .map_or(DetailTab::Info, DetailTab::default_for);
        }
        cx.notify();
    }

    /// Keep the OS-appearance observer alive.
    pub fn set_appearance_obs(&mut self, sub: Subscription) {
        self.appearance_obs = Some(sub);
    }

    /// Drop `active_tab` when the current device no longer exposes that section.
    fn reconcile_active_tab(&mut self, cx: &Context<Self>) {
        if !matches!(self.nav, SidebarNav::Device(_)) {
            return;
        }
        let Some(record) = cx
            .try_global::<AppState>()
            .and_then(AppState::current_record)
        else {
            return;
        };
        let tabs = DetailTab::tabs_for(record);
        if !tabs.contains(&self.active_tab) {
            self.active_tab = tabs.first().copied().unwrap_or(DetailTab::Info);
        }
    }

    fn reconcile_nav(&mut self, cx: &Context<Self>) {
        if let SidebarNav::Device(idx) = self.nav {
            let len = cx
                .try_global::<AppState>()
                .map_or(0, |s| s.device_list.len());
            if len == 0 {
                self.nav = SidebarNav::Devices;
            } else if idx >= len {
                self.nav = SidebarNav::Devices;
            }
        }
    }

    /// Query macOS for the live Accessibility trust state and mirror it into
    /// [`AppState`]. Unsigned or rebuilt binaries often keep a stale `false` in
    /// state while System Settings already lists the installed `.app` as allowed.
    fn sync_accessibility(cx: &mut Context<Self>) -> bool {
        let live = Hook::has_accessibility();
        if cx.has_global::<AppState>() {
            cx.update_global::<AppState, _>(|state, _| {
                state.accessibility_granted = live;
            });
        }
        live
    }

    fn accessibility_gate(pal: Palette, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .size_full()
            .bg(pal.window_bg)
            .text_color(pal.text_primary)
            .items_center()
            .justify_center()
            .p_8()
            .child(
                v_flex()
                    .rounded_2xl()
                    .bg(pal.card_bg)
                    .shadow(card_shadow())
                    .p_10()
                    .gap_4()
                    .items_center()
                    .max_w(px(480.))
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size_8()
                            .text_color(rgb(theme::STATUS_CONNECTING)),
                    )
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr!("Accessibility permission required")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_center()
                            .text_color(pal.text_muted)
                            .child(tr!(
                                "OpenLogi captures mouse buttons (Back / Forward / gesture button) \
                                 through the system Accessibility permission and runs the actions you \
                                 bind. Features that talk to the device directly — DPI, SmartShift — \
                                 are unaffected."
                            )),
                    )
                    .child(
                        div()
                            .id("open-accessibility")
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(theme::ACCENT_BLUE))
                            .text_color(rgb(0x00ff_ffff))
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(Icon::new(IconName::Settings))
                                    .child(tr!("Open System Settings to grant access")),
                            )
                            .on_click(|_, _, _| open_accessibility_settings()),
                    )
                    .child(div().text_xs().text_color(pal.text_muted).child(tr!(
                        "Takes effect automatically once granted — no restart needed."
                    )))
                    .child(
                        div()
                            .id("recheck-accessibility")
                            .text_xs()
                            .text_color(rgb(theme::ACCENT_BLUE))
                            .cursor_pointer()
                            .hover(|s| s.text_color(pal.text_primary))
                            .child(tr!("Check again"))
                            .on_click(cx.listener(|_, _, _, cx| {
                                let _ = Self::sync_accessibility(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("skip-accessibility")
                            .text_xs()
                            .text_color(pal.text_muted)
                            .cursor_pointer()
                            .hover(|s| s.text_color(pal.text_primary))
                            .child(tr!("Not now (use DPI and other features only)"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.accessibility_dismissed = true;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}

fn open_accessibility_settings() {
    use crate::platform::permissions::{self, Permission};
    // Single source of the prompt + System Settings deep link, shared with the
    // Settings window's Permissions row.
    permissions::open_pane(Permission::Accessibility);
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = theme::palette(cx);

        let granted = Self::sync_accessibility(cx);
        if !granted && !self.accessibility_dismissed {
            window.set_window_title("OpenLogi");
            return Self::accessibility_gate(pal, cx);
        }

        let has_device = cx
            .try_global::<AppState>()
            .is_some_and(|s| !s.device_list.is_empty());
        let scanning = cx.try_global::<AppState>().is_some_and(|s| s.scanning);
        self.reconcile_nav(cx);
        if matches!(self.nav, SidebarNav::Device(_)) {
            self.reconcile_active_tab(cx);
        }

        window.set_window_title(&main_window_title(self.nav, cx));

        let main_pane = if let Some(section) = self.nav.settings_section() {
            embedded_settings_content(section, &self.language_select, pal, cx).into_any_element()
        } else {
            match self.nav {
                SidebarNav::Devices => {
                    if has_device {
                        device_gallery(cx).into_any_element()
                    } else {
                        device_empty_state(pal, scanning)
                    }
                }
                SidebarNav::Device(_) => {
                    if has_device
                        && cx
                            .try_global::<AppState>()
                            .and_then(AppState::current_record)
                            .is_some()
                    {
                        detail_shell(
                            &self.mouse_model,
                            &self.dpi_panel,
                            &self.lighting_panel,
                            self.active_tab,
                            pal,
                            cx,
                        )
                        .into_any_element()
                    } else {
                        device_empty_state(pal, scanning)
                    }
                }
                SidebarNav::General | SidebarNav::Permissions | SidebarNav::Language => {
                    device_empty_state(pal, scanning)
                }
            }
        };

        v_flex()
            .size_full()
            .text_color(pal.text_primary)
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &Zoom, window, _| window.zoom_window())
            .child(
                h_flex()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .child(app_sidebar(self.nav, pal, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .self_stretch()
                            .bg(pal.window_bg)
                            .child(main_pane),
                    ),
            )
            .child(footer(pal, granted))
            .into_any_element()
    }
}

/// Top-level sidebar section — a menu (one or more items), a muted label, or a gap spacer.
#[derive(Clone)]
enum MainSidebarSection {
    /// A group of nav items. Wrapped in `py_1` for breathing room.
    Menu(SidebarMenu),
    /// Muted section header text (e.g. "Settings").
    Label(SharedString),
    /// Transparent vertical gap (px).
    Spacer(u32),
}

impl Collapsible for MainSidebarSection {
    fn is_collapsed(&self) -> bool {
        match self {
            Self::Menu(menu) => menu.is_collapsed(),
            Self::Label(_) | Self::Spacer(_) => false,
        }
    }

    fn collapsed(self, collapsed: bool) -> Self {
        match self {
            Self::Menu(menu) => Self::Menu(menu.collapsed(collapsed)),
            other => other,
        }
    }
}

impl SidebarItem for MainSidebarSection {
    fn render(
        self,
        id: impl Into<gpui::ElementId>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> impl IntoElement {
        match self {
            Self::Menu(menu) => div()
                .py_1()
                .child(menu.render(id, window, cx))
                .into_any_element(),
            Self::Label(text) => {
                let _ = id;
                div()
                    .px_3()
                    .pt_2()
                    .pb_0p5()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::palette(cx).text_muted)
                    .child(text)
                    .into_any_element()
            }
            Self::Spacer(h) => {
                let _ = id;
                div().h(px(h as f32)).into_any_element()
            }
        }
    }
}

/// macOS System Settings–style sidebar (gpui-component [`Sidebar`] + grouped menus).
fn app_sidebar(nav: SidebarNav, pal: Palette, cx: &mut Context<AppView>) -> impl IntoElement {
    let view = cx.entity();
    let devices_view = view.clone();

    let nav_item =
        |section: SidebarNav, label: SharedString, icon: IconName, color: gpui::Rgba| {
            let active = nav == section;
            let view = view.clone();
            MainSidebarSection::Menu(
                SidebarMenu::new().child(
                    SidebarMenuItem::new(label)
                        .icon(Icon::new(icon).text_color(color))
                        .active(active)
                        .on_click(move |_, _, cx| {
                            view.update(cx, |this, cx| this.set_nav(section, cx));
                        }),
                ),
            )
        };

    Sidebar::new("main-sidebar")
        .h_full()
        .flex_shrink_0()
        .bg(pal.sidebar_bg)
        .border_r_1()
        .border_color(pal.sidebar_border)
        .collapsible(SidebarCollapsible::None)
        .header(
            div()
                .h(px(44.))
                .w_full()
                .window_control_area(WindowControlArea::Drag),
        )
        .footer(add_device_sidebar_button(pal))
        .child(MainSidebarSection::Menu(
            SidebarMenu::new().child(
                SidebarMenuItem::new(tr!("Devices"))
                    .icon(Icon::new(IconName::Cpu).text_color(rgb(theme::ACCENT_BLUE)))
                    .active(nav == SidebarNav::Devices)
                    .on_click(move |_, _, cx| {
                        devices_view.update(cx, |this, cx| this.set_nav(SidebarNav::Devices, cx));
                    }),
            ),
        ))
        .child(MainSidebarSection::Spacer(16))
        .child(MainSidebarSection::Label(tr!("Settings")))
        .child(nav_item(SidebarNav::General, tr!("General"), IconName::Settings, rgb(0x006b_7280)))
        .child(MainSidebarSection::Spacer(4))
        .child(nav_item(SidebarNav::Permissions, tr!("Permissions"), IconName::Info, rgb(0x00f9_7316)))
        .child(MainSidebarSection::Spacer(4))
        .child(nav_item(SidebarNav::Language, tr!("Language"), IconName::Globe, rgb(0x0022_c55e)))
}

/// Gap between preview cards in the grid.
const GALLERY_GAP: f32 = 24.;

/// Outer padding of the device grid (cards breathe from the pane edges).
const GALLERY_PAD: f32 = 32.;

/// Devices overview: a wrapping grid of device cards, one per device.
fn device_gallery(cx: &mut Context<AppView>) -> impl IntoElement {
    let active_idx = cx
        .try_global::<AppState>()
        .map_or(0, |s| s.current_device.min(s.device_list.len().saturating_sub(1)));

    let records: Vec<DeviceRecord> = cx
        .try_global::<AppState>()
        .map_or_else(Vec::new, |s| s.device_list.clone());

    let view = cx.entity();
    let pal = theme::palette(cx);

    let cards: Vec<AnyElement> = records
        .into_iter()
        .enumerate()
        .map(|(idx, record)| {
            let view = view.clone();
            device_card(&record, idx == active_idx, pal)
                .id(("device-card", idx))
                .cursor_pointer()
                .hover(move |s| s.bg(pal.card_hover_bg).shadow(card_shadow_hover()))
                .on_click(move |_, _, cx| {
                    view.update(cx, |this, cx| this.set_nav(SidebarNav::Device(idx), cx));
                })
                .into_any_element()
        })
        .collect();

    v_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .bg(pal.window_bg)
        .overflow_y_scrollbar()
        .child(
            div()
                .flex()
                .flex_wrap()
                .justify_center()
                .content_start()
                .gap(px(GALLERY_GAP))
                .p(px(GALLERY_PAD))
                .children(cards),
        )
}

fn device_card(record: &DeviceRecord, _active: bool, pal: Palette) -> Div {
    v_flex()
        .w(px(theme::GALLERY_CARD_W))
        .flex_shrink_0()
        .items_center()
        .gap_3()
        .p_5()
        .rounded_2xl()
        .bg(pal.card_bg)
        .shadow(card_shadow())
        .child(
            div()
                .w_full()
                .h(px(theme::GALLERY_PHOTO_H))
                .flex()
                .items_center()
                .justify_center()
                .child(device_image(record, pal)),
        )
        .child(
            v_flex()
                .w_full()
                .gap_1()
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(record.display_name.clone()),
                        )
                        .child(device_status_row(record.online, record.battery.as_ref(), pal)),
                )
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_xs()
                        .text_color(pal.text_muted)
                        .child(format!(
                            "{} · slot {}",
                            kind_label(record.kind),
                            record.slot
                        )),
                ),
        )
}

/// The device photo, scaled to fit its preview card, or a neutral placeholder.
fn device_image(record: &DeviceRecord, pal: Palette) -> AnyElement {
    match record
        .asset
        .as_ref()
        .and_then(|a| a.hero_image_path.clone())
    {
        Some(path) => img(path).max_w_full().max_h_full().into_any_element(),
        None => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(Icon::new(IconName::Cpu).size_8().text_color(pal.text_muted))
            .into_any_element(),
    }
}

/// Connection status pill with inline battery readout:
/// `[dot] Connected · [battery icon] 80%`
fn device_status_row(
    online: bool,
    battery: Option<&BatteryInfo>,
    pal: Palette,
) -> impl IntoElement {
    let (label, color) = if online {
        (tr!("Connected"), theme::STATUS_CONNECTED)
    } else {
        (tr!("Offline"), theme::STATUS_OFFLINE)
    };
    let icon = battery
        .map(battery_icon)
        .unwrap_or(IconName::Battery);
    let percentage = battery
        .map(|b| format!("{}%", b.percentage))
        .unwrap_or_else(|| "—".to_string());

    h_flex()
        .flex_shrink_0()
        .gap_1()
        .items_center()
        .rounded_full()
        .bg(pal.card_bg)
        .border_1()
        .border_color(pal.sidebar_border)
        .px_2()
        .py_1()
        .text_xs()
        .text_color(pal.text_muted)
        .child(div().size_1p5().rounded_full().bg(rgb(color)))
        .child(label)
        .child("·")
        .child(Icon::new(icon).size_3())
        .child(percentage)
}

/// Pick the battery glyph from charge state first, then discrete level.
fn battery_icon(b: &BatteryInfo) -> IconName {
    match b.status {
        BatteryStatus::Charging | BatteryStatus::ChargingSlow => IconName::BatteryCharging,
        BatteryStatus::Full => IconName::BatteryFull,
        BatteryStatus::Error => IconName::BatteryWarning,
        BatteryStatus::Discharging | BatteryStatus::Unknown => match b.level {
            BatteryLevel::Critical => IconName::BatteryWarning,
            BatteryLevel::Low => IconName::BatteryLow,
            BatteryLevel::Good => IconName::BatteryMedium,
            BatteryLevel::Full => IconName::BatteryFull,
            BatteryLevel::Unknown => IconName::Battery,
        },
    }
}

/// Detail pane: large title + tabbed sections for the selected device.
fn detail_shell(
    mouse_model: &Entity<MouseModelView>,
    dpi_panel: &Entity<DpiPanel>,
    lighting_panel: &Entity<LightingPanel>,
    active: DetailTab,
    pal: Palette,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .child(detail_title_bar(pal, cx))
        .child(detail_content(
            mouse_model,
            dpi_panel,
            lighting_panel,
            active,
            pal,
            cx,
        ))
}

/// Content-area header (macOS Settings large title + status).
fn detail_title_bar(pal: Palette, cx: &mut Context<AppView>) -> impl IntoElement {
    let record = cx
        .try_global::<AppState>()
        .and_then(AppState::current_record)
        .cloned();
    h_flex()
        .w_full()
        .px_6()
        .pt_6()
        .pb_3()
        .gap_3()
        .items_end()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xl()
                .font_weight(FontWeight::BOLD)
                .child(
                    record
                        .as_ref()
                        .map_or_else(|| tr!("Device").to_string(), |r| r.display_name.clone()),
                ),
        )
        .when_some(record, |this, r| {
            this.child(device_status_row(r.online, r.battery.as_ref(), pal))
        })
}

/// Pick the battery glyph from charge state first (charging / full / error),
/// then fall back to the discrete charge level for a plain discharge.
fn main_window_title(nav: SidebarNav, cx: &Context<AppView>) -> SharedString {
    if matches!(nav, SidebarNav::Devices) {
        return SharedString::from("OpenLogi");
    }
    if let Some(section) = nav.settings_section() {
        let title = match section {
            settings_pages::SettingsSection::General => tr!("General"),
            settings_pages::SettingsSection::Permissions => tr!("Permissions"),
            settings_pages::SettingsSection::Language => tr!("Language"),
        };
        return SharedString::from(format!("OpenLogi — {}", title));
    }
    cx.try_global::<AppState>()
        .and_then(AppState::current_record)
        .map_or_else(
            || SharedString::from("OpenLogi"),
            |record| SharedString::from(format!("OpenLogi — {}", record.display_name)),
        )
}

/// The device-detail body: a tab bar over the active device's sections (which
/// vary by kind — see [`DetailTab::tabs_for`]), with the active section filling
/// the rest of the screen.
fn detail_content(
    mouse_model: &Entity<MouseModelView>,
    dpi_panel: &Entity<DpiPanel>,
    lighting_panel: &Entity<LightingPanel>,
    active: DetailTab,
    pal: Palette,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let tabs = cx
        .try_global::<AppState>()
        .and_then(AppState::current_record)
        .map_or_else(|| vec![DetailTab::Info], DetailTab::tabs_for);
    let active = if tabs.contains(&active) {
        active
    } else {
        tabs.first().copied().unwrap_or(DetailTab::Info)
    };
    let content = match active {
        DetailTab::Buttons => buttons_tab(mouse_model).into_any_element(),
        DetailTab::Pointer => pointer_tab(dpi_panel, pal).into_any_element(),
        DetailTab::Lighting => lighting_tab(lighting_panel, pal).into_any_element(),
        DetailTab::Info => device_tab(pal, cx).into_any_element(),
    };
    v_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .child(detail_tab_bar(&tabs, active, cx))
        .child(
            div()
                .flex_1()
                .w_full()
                .min_h_0()
                .overflow_hidden()
                .child(content),
        )
}

/// The detail screen's tab bar, built from the active device's tab set. Clicking
/// a tab swaps the active section.
fn detail_tab_bar(
    tabs: &[DetailTab],
    active: DetailTab,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let active_ix = tabs.iter().position(|t| *t == active).unwrap_or(0);
    // Owned copy so the click handler can map a clicked index back to its tab
    // without borrowing the caller's slice.
    let order = tabs.to_vec();
    div().w_full().px_6().pt_2().child(
        TabBar::new("detail-tabs")
            .underline()
            .w_full()
            .selected_index(active_ix)
            .children(tabs.iter().map(|t| t.label()))
            .on_click(cx.listener(move |this, ix: &usize, _, cx| {
                this.active_tab = order.get(*ix).copied().unwrap_or(DetailTab::Info);
                cx.notify();
            })),
    )
}

/// Buttons tab: the mouse model with clickable hotspots, centred with a max
/// width so it doesn't stretch across a wide window.
fn buttons_tab(mouse_model: &Entity<MouseModelView>) -> impl IntoElement {
    h_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .justify_center()
        .p_6()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .max_w(px(760.))
                .child(mouse_model.clone()),
        )
}

/// Pointer tab: the DPI panel in a titled card.
fn pointer_tab(dpi_panel: &Entity<DpiPanel>, pal: Palette) -> impl IntoElement {
    v_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .min_w_0()
        .bg(pal.window_bg)
        .overflow_y_scrollbar()
        .p_6()
        .child(panel_card(
            tr!("Pointer tuning"),
            IconName::Settings,
            pal,
            dpi_panel.clone().into_any_element(),
        ))
}

/// Lighting tab: the RGB controls (swatches, on/off, brightness) in a titled
/// card. Only reached for wired keyboards — see [`supports_lighting`].
fn lighting_tab(lighting_panel: &Entity<LightingPanel>, pal: Palette) -> impl IntoElement {
    v_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .bg(pal.window_bg)
        .items_center()
        .overflow_y_scrollbar()
        .p_6()
        .child(div().w_full().max_w(px(560.)).child(panel_card(
            tr!("Lighting"),
            IconName::Palette,
            pal,
            lighting_panel.clone().into_any_element(),
        )))
}

/// Info tab: device details and configuration cards stacked.
fn device_tab(pal: Palette, cx: &mut Context<AppView>) -> impl IntoElement {
    v_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .bg(pal.window_bg)
        .overflow_y_scrollbar()
        .p_6()
        .gap_4()
        .child(device_details_card(pal, cx))
        .child(configuration_card(pal, cx))
}

fn device_details_card(pal: Palette, cx: &mut Context<AppView>) -> impl IntoElement {
    let content = cx
        .try_global::<AppState>()
        .and_then(AppState::current_record)
        .cloned()
        .map_or_else(
            || {
                div()
                    .text_sm()
                    .text_color(pal.text_muted)
                    .child(tr!("No active device"))
                    .into_any_element()
            },
            |record| {
                v_flex()
                    .w_full()
                    .gap_3()
                    .child(device_summary(
                        &record.display_name,
                        record.kind,
                        record.online,
                        record.battery.as_ref(),
                        pal,
                    ))
                    .child(device_description_list(record))
                    .into_any_element()
            },
        );

    panel_card(tr!("Device details"), IconName::Info, pal, content)
}

fn configuration_card(pal: Palette, cx: &mut Context<AppView>) -> impl IntoElement {
    let (binding_count, gesture_count, preset_count, app_profile) = cx
        .try_global::<AppState>()
        .map_or((0, 0, 0, tr!("Default profile").to_string()), |state| {
            (
                state.button_bindings.len(),
                state.gesture_bindings.len(),
                state.dpi_presets().len(),
                state
                    .current_app_bundle
                    .clone()
                    .unwrap_or_else(|| tr!("Default profile").to_string()),
            )
        });

    let content = v_flex()
        .w_full()
        .gap_3()
        .child(
            DescriptionList::new()
                .columns(1)
                .label_width(px(118.))
                .bordered(false)
                .child(DescriptionItem::new(tr!("Active profile")).value(app_profile))
                .child(
                    DescriptionItem::new(tr!("Button bindings")).value(binding_count.to_string()),
                )
                .child(
                    DescriptionItem::new(tr!("Gesture bindings")).value(gesture_count.to_string()),
                )
                .child(DescriptionItem::new(tr!("DPI presets")).value(preset_count.to_string())),
        )
        .child(
            h_flex()
                .gap_2()
                .pt_1()
                .child(sidebar_action(
                    "right-panel-settings",
                    IconName::Settings,
                    tr!("Settings"),
                    pal,
                    |_event, _window, cx| settings::open_section(SidebarNav::General, cx),
                ))
                .child(sidebar_action(
                    "right-panel-config-folder",
                    IconName::Folder,
                    tr!("Config folder"),
                    pal,
                    |_event, _window, cx| {
                        if let Ok(path) = openlogi_core::paths::config_dir() {
                            cx.open_url(&file_url(&path));
                        }
                    },
                )),
        )
        .into_any_element();

    panel_card(tr!("Configuration"), IconName::Folder, pal, content)
}

fn device_summary(
    name: &str,
    kind: DeviceKind,
    online: bool,
    battery: Option<&BatteryInfo>,
    pal: Palette,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .justify_between()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name.to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(pal.text_muted)
                        .child(kind_label(kind)),
                ),
        )
        .child(device_status_row(online, battery, pal))
}

fn device_description_list(record: crate::state::DeviceRecord) -> impl IntoElement {
    let mut items = vec![
        DescriptionItem::new(tr!("Connection")).value(route_label(record.route.as_ref())),
        DescriptionItem::new(tr!("Slot")).value(record.slot.to_string()),
        DescriptionItem::new(tr!("Device key")).value(record.config_key.clone()),
    ];
    if let Some(serial) = record.serial_number {
        items.push(DescriptionItem::new(tr!("Serial")).value(serial));
    }

    DescriptionList::new()
        .columns(1)
        .label_width(px(100.))
        .bordered(false)
        .children(items)
}

fn panel_card(
    title: SharedString,
    icon: IconName,
    pal: Palette,
    content: AnyElement,
) -> impl IntoElement {
    div()
        .w_full()
        .max_w_full()
        .min_w_0()
        .rounded_xl()
        .bg(pal.card_bg)
        .shadow(card_shadow())
        .p_5()
        .child(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .when(!title.is_empty(), |this| {
                    this.child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .text_color(pal.text_primary)
                            .child(Icon::new(icon).size_4().text_color(pal.text_muted))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            ),
                    )
                })
                .child(content),
        )
}

/// Sidebar footer control — white card surface in light mode, raised dark
/// surface in dark mode.
fn add_device_sidebar_button(pal: Palette) -> impl IntoElement {
    h_flex()
        .id("sidebar-add-device")
        .w_full()
        .h(px(32.))
        .justify_center()
        .items_center()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(pal.sidebar_border)
        .bg(pal.card_bg)
        .shadow(card_shadow())
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(pal.text_primary)
        .cursor_pointer()
        .hover(move |s| s.bg(pal.surface_hover).shadow(card_shadow_hover()))
        .child(Icon::new(IconName::Plus).size_4())
        .child(tr!("Add Device"))
        .on_click(|_, _, cx| crate::windows::add_device::open(cx))
}

fn sidebar_action(
    id: &'static str,
    icon: IconName,
    label: SharedString,
    pal: Palette,
    handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    h_flex()
        .id(id)
        .flex_1()
        .justify_center()
        .items_center()
        .gap_1()
        .rounded_md()
        .border_1()
        .border_color(pal.border)
        .bg(pal.surface)
        .px_2()
        .py_1()
        .text_xs()
        .text_color(pal.text_primary)
        .cursor_pointer()
        .hover(move |s| s.bg(pal.surface_hover))
        .child(Icon::new(icon).size_3())
        .child(label)
        .on_click(handler)
        .into_any_element()
}

fn route_label(route: Option<&DeviceRoute>) -> String {
    match route {
        Some(DeviceRoute::Bolt { .. }) => tr!("Bolt receiver").to_string(),
        Some(DeviceRoute::Direct { .. }) => tr!("Direct connection").to_string(),
        None => tr!("Unavailable").to_string(),
    }
}

fn kind_label(kind: DeviceKind) -> String {
    match kind {
        DeviceKind::Mouse => tr!("Mouse").to_string(),
        DeviceKind::Keyboard => tr!("Keyboard").to_string(),
        DeviceKind::Numpad => tr!("Numpad").to_string(),
        DeviceKind::Presenter => tr!("Presenter").to_string(),
        DeviceKind::Remote => tr!("Remote").to_string(),
        DeviceKind::Trackball => tr!("Trackball").to_string(),
        DeviceKind::Touchpad => tr!("Touchpad").to_string(),
        DeviceKind::Tablet => tr!("Tablet").to_string(),
        DeviceKind::Gamepad => tr!("Gamepad").to_string(),
        DeviceKind::Joystick => tr!("Joystick").to_string(),
        DeviceKind::Headset => tr!("Headset").to_string(),
        DeviceKind::Unknown => tr!("Device").to_string(),
    }
}

fn file_url(path: &std::path::Path) -> String {
    format!("file://{}", path.to_string_lossy().replace(' ', "%20"))
}

/// Body shown when no device is connected. The inventory watcher keeps polling
/// (every 2 s) and `AppView`'s `AppState` observer swaps the device UI back in
/// the moment one appears, so this is purely a wait-and-pair placeholder.
fn device_empty_state(pal: Palette, scanning: bool) -> AnyElement {
    v_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .bg(pal.window_bg)
        .items_center()
        .justify_center()
        .p_8()
        .child(
            v_flex()
                .rounded_2xl()
                .bg(pal.card_bg)
                .shadow(card_shadow())
                .p_10()
                .gap_4()
                .items_center()
                .max_w(px(480.))
                .child(
                    Icon::new(IconName::Search)
                        .size_8()
                        .text_color(pal.text_muted),
                )
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(if scanning {
                            tr!("Scanning for devices…")
                        } else {
                            tr!("No devices connected")
                        }),
                )
                .child(
                    div()
                        .max_w(px(360.))
                        .text_sm()
                        .text_center()
                        .text_color(pal.text_muted)
                        .child(tr!(
                            "Plug in or pair a supported Logitech device — it'll show up here automatically. For direct Bluetooth connections, pair in your computer's bluetooth settings."
                        )),
                )
                .child(
                    Button::new("empty-add-device")
                        .primary()
                        .label(tr!("Add Device"))
                        .on_click(|_, _, cx| crate::windows::add_device::open(cx)),
                )
                .child(div().max_w(px(360.)).text_xs().text_center().text_color(pal.text_muted).child(tr!(
                    "Using Logi Options+? Quit it first — both apps compete for HID++ access."
                ))),
        )
        .into_any_element()
}

/// Footer status bar: passive state only. Left — Accessibility permission;
/// right — app version. Add Device and Settings live in the sidebar toolbar
/// (and the menu bar); About stays in the app menu.
fn footer(pal: Palette, granted: bool) -> impl IntoElement {
    h_flex()
        .h(px(FOOTER_H))
        .w_full()
        .px_5()
        .gap_4()
        .items_center()
        .justify_between()
        .bg(pal.window_bg)
        .border_t_1()
        .border_color(pal.sidebar_border)
        .child(accessibility_status(pal, granted))
        .child(
            div()
                .text_xs()
                .text_color(pal.text_muted)
                .child(concat!("v", env!("CARGO_PKG_VERSION"))),
        )
}

/// Footer Accessibility-permission indicator. Granted → a muted green-dot
/// status; not granted → an amber-dot affordance that requests the grant on
/// click (the native prompt + System Settings, via [`open_accessibility_settings`]).
fn accessibility_status(pal: Palette, granted: bool) -> AnyElement {
    if granted {
        // Reassurance only — kept deliberately quiet: a small dimmed dot and
        // muted text that recede until something is actually wrong.
        h_flex()
            .gap_1p5()
            .items_center()
            .text_xs()
            .text_color(pal.text_muted)
            .child(
                div()
                    .size_1p5()
                    .rounded_full()
                    .bg(rgb(theme::STATUS_CONNECTED)),
            )
            .child(div().child(tr!("Accessibility granted")))
            .into_any_element()
    } else {
        // The state that needs attention — full-strength text, an amber dot,
        // and a click target that requests the grant.
        h_flex()
            .id("footer-accessibility")
            .gap_2()
            .items_center()
            .text_xs()
            .text_color(pal.text_primary)
            .cursor_pointer()
            .child(
                div()
                    .size_2()
                    .rounded_full()
                    .bg(rgb(theme::STATUS_CONNECTING)),
            )
            .child(div().child(tr!("Accessibility not granted · click to grant")))
            .on_click(|_, _, _| open_accessibility_settings())
            .into_any_element()
    }
}
