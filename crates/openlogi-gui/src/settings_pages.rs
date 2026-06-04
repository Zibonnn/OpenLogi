//! Shared settings page builders and embedded settings content for the main window.

use gpui::{
    App, AppContext as _, BorrowAppContext as _, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    px, rgb,
};
use gpui_component::{
    IconName, IndexPath, Sizable as _, h_flex,
    scroll::ScrollableElement as _,
    select::{Select, SelectEvent, SelectItem, SelectState},
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    switch::Switch,
    v_flex,
};

use crate::platform::permissions::{self, Permission, PermissionStatus};
use crate::state::AppState;
use crate::theme::{self, Palette};

#[derive(Clone)]
pub struct LanguageOption {
    pub label: &'static str,
    pub value: &'static str,
    pub localize_label: bool,
}

impl SelectItem for LanguageOption {
    type Value = &'static str;

    fn title(&self) -> SharedString {
        if self.localize_label {
            SharedString::from(rust_i18n::t!("Follow system").into_owned())
        } else {
            SharedString::from(self.label)
        }
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

pub fn language_options() -> Vec<LanguageOption> {
    let mut options = vec![LanguageOption {
        label: "Follow system",
        value: "",
        localize_label: true,
    }];
    options.extend(
        crate::i18n::SUPPORTED
            .iter()
            .map(|(code, name)| LanguageOption {
                label: name,
                value: code,
                localize_label: false,
            }),
    );
    options
}

pub fn selected_language_index(current: Option<&str>, options: &[LanguageOption]) -> IndexPath {
    let value = current.unwrap_or_default();
    let row = options
        .iter()
        .position(|option| option.value == value)
        .unwrap_or_default();
    IndexPath::default().row(row)
}

pub fn new_language_select(
    window: &mut Window,
    cx: &mut App,
) -> Entity<SelectState<Vec<LanguageOption>>> {
    let current = cx
        .try_global::<AppState>()
        .and_then(|s| s.app_settings().language.clone());
    let options = language_options();
    let selected = selected_language_index(current.as_deref(), &options);
    cx.new(|cx| SelectState::new(options, Some(selected), window, cx))
}

pub fn on_language_select(
    select: &Entity<SelectState<Vec<LanguageOption>>>,
    event: &SelectEvent<Vec<LanguageOption>>,
    _: &mut Window,
    cx: &mut App,
) {
    let SelectEvent::Confirm(_) = event;
    let language = select
        .read(cx)
        .selected_value()
        .copied()
        .filter(|code| !code.is_empty())
        .map(ToOwned::to_owned);

    cx.update_global::<AppState, _>(|s, _| s.set_language(language));
    cx.refresh_windows();
    crate::app_menu::rebuild(cx);
    #[cfg(target_os = "macos")]
    crate::platform::tray::request_refresh();
}

pub fn general_page() -> SettingPage {
    let group = SettingGroup::new()
        .item(
            SettingItem::new(
                tr!("Launch at login"),
                SettingField::switch(
                    |cx| {
                        cx.try_global::<AppState>()
                            .is_some_and(|s| s.app_settings().launch_at_login)
                    },
                    |enabled, cx| {
                        cx.update_global::<AppState, _>(move |s, _| {
                            s.set_launch_at_login(enabled);
                        });
                        cx.refresh_windows();
                    },
                ),
            )
            .description(tr!(
                "Automatically start OpenLogi when you log in to macOS."
            )),
        )
        .item(
            SettingItem::new(
                tr!("Check for updates"),
                SettingField::switch(
                    |cx| {
                        cx.try_global::<AppState>()
                            .is_some_and(|s| s.app_settings().check_for_updates)
                    },
                    |enabled, cx| {
                        cx.update_global::<AppState, _>(move |s, _| {
                            s.set_check_for_updates(enabled);
                        });
                        cx.refresh_windows();
                    },
                ),
            )
            .description(tr!(
                "Check once per launch for a new version (query only — no automatic download)."
            )),
        );

    #[cfg(target_os = "macos")]
    let group = group.item(
        SettingItem::new(
            tr!("Show in menu bar"),
            SettingField::switch(
                |cx| {
                    cx.try_global::<AppState>()
                        .is_some_and(|s| s.app_settings().show_in_menu_bar)
                },
                |enabled, cx| {
                    cx.update_global::<AppState, _>(move |s, _| {
                        s.set_show_in_menu_bar(enabled);
                    });
                    cx.refresh_windows();
                },
            ),
        )
        .description(tr!(
            "Keep OpenLogi's icon in the menu bar. When off, it stays in the Dock instead."
        )),
    );

    SettingPage::new(tr!("General"))
        .icon(IconName::Settings)
        .resettable(false)
        .group(group)
}

pub fn permissions_page(pal: Palette) -> SettingPage {
    SettingPage::new(tr!("Permissions"))
        .icon(IconName::Info)
        .resettable(false)
        .group(
            SettingGroup::new()
                .item(permission_item(
                    "perm-accessibility",
                    tr!("Accessibility"),
                    tr!("Needed for gesture and button remapping (event tap)."),
                    Permission::Accessibility,
                    |cx| {
                        if cx
                            .try_global::<AppState>()
                            .is_some_and(|s| s.accessibility_granted)
                        {
                            PermissionStatus::Granted
                        } else {
                            PermissionStatus::Denied
                        }
                    },
                    pal,
                ))
                .item(permission_item(
                    "perm-input-monitoring",
                    tr!("Input Monitoring"),
                    tr!("Needed to read HID++ data, including Bluetooth-direct mice."),
                    Permission::InputMonitoring,
                    |_| permissions::input_monitoring(),
                    pal,
                ))
                .item(permission_item(
                    "perm-bluetooth",
                    tr!("Bluetooth"),
                    tr!("Allows OpenLogi to use CoreBluetooth (not required for HID access)."),
                    Permission::Bluetooth,
                    |_| permissions::bluetooth(),
                    pal,
                )),
        )
}

fn permission_item(
    id: &'static str,
    title: SharedString,
    description: SharedString,
    permission: Permission,
    status: impl Fn(&App) -> PermissionStatus + 'static,
    pal: Palette,
) -> SettingItem {
    SettingItem::new(
        title,
        SettingField::render(move |_, _, cx| permission_field(id, status(cx), permission, pal)),
    )
    .description(description)
}

pub fn language_page(language_select: Entity<SelectState<Vec<LanguageOption>>>) -> SettingPage {
    SettingPage::new(tr!("Language"))
        .icon(IconName::Globe)
        .resettable(false)
        .group(
            SettingGroup::new().item(
                SettingItem::new(
                    tr!("Language"),
                    SettingField::render(move |_, _, _| {
                        language_select_field(language_select.clone())
                    }),
                )
                .description(tr!("Choose the interface language.")),
            ),
        )
}

fn permission_field(
    id: &'static str,
    status: PermissionStatus,
    permission: Permission,
    pal: Palette,
) -> impl IntoElement {
    h_flex()
        .flex_shrink_0()
        .items_center()
        .gap_3()
        .child(status_badge(status))
        .child(
            div()
                .id(id)
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(pal.border)
                .text_xs()
                .cursor_pointer()
                .hover(move |s| s.bg(pal.surface_hover))
                .child(tr!("Open"))
                .on_click(move |_, _, _| permissions::open_pane(permission)),
        )
}

fn status_badge(status: PermissionStatus) -> impl IntoElement {
    let (label, color) = match status {
        PermissionStatus::Granted => (tr!("Granted"), theme::STATUS_CONNECTED),
        PermissionStatus::Denied => (tr!("Not granted"), theme::STATUS_CONNECTING),
        PermissionStatus::Unknown => (tr!("Unknown"), theme::STATUS_OFFLINE),
    };
    div().text_xs().text_color(rgb(color)).child(label)
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "built inside an `Fn` render closure"
)]
fn language_select_field(
    language_select: Entity<SelectState<Vec<LanguageOption>>>,
) -> impl IntoElement {
    div().flex_shrink_0().w(px(220.)).h_6().child(
        Select::new(&language_select)
            .small()
            .w(px(220.))
            .menu_width(px(220.)),
    )
}

/// Which settings page to show in the main window detail pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    General,
    Permissions,
    Language,
}

/// Render one settings page in the main window content area.
pub fn embedded_settings_content(
    section: SettingsSection,
    language_select: &Entity<SelectState<Vec<LanguageOption>>>,
    pal: Palette,
    cx: &mut App,
) -> impl IntoElement {
    match section {
        SettingsSection::General => embedded_general_settings(pal, cx).into_any_element(),
        SettingsSection::Permissions => embedded_permissions_settings(pal, cx).into_any_element(),
        SettingsSection::Language => {
            embedded_language_settings(language_select.clone(), pal).into_any_element()
        }
    }
}

fn embedded_general_settings(pal: Palette, cx: &mut App) -> impl IntoElement {
    let (launch_at_login, check_for_updates, show_in_menu_bar) = cx
        .try_global::<AppState>()
        .map_or((false, false, false), |s| {
            let settings = s.app_settings();
            (
                settings.launch_at_login,
                settings.check_for_updates,
                settings.show_in_menu_bar,
            )
        });

    #[cfg(not(target_os = "macos"))]
    let _ = show_in_menu_bar;

    let rows = v_flex()
        .gap_0()
        .child(settings_switch_row(
            "settings-launch-login",
            tr!("Launch at login"),
            tr!("Automatically start OpenLogi when you log in to macOS."),
            launch_at_login,
            |enabled, cx| {
                cx.update_global::<AppState, _>(move |s, _| s.set_launch_at_login(enabled));
                cx.refresh_windows();
            },
            pal,
        ))
        .child(settings_switch_row(
            "settings-check-updates",
            tr!("Check for updates"),
            tr!("Check once per launch for a new version (query only — no automatic download)."),
            check_for_updates,
            |enabled, cx| {
                cx.update_global::<AppState, _>(move |s, _| s.set_check_for_updates(enabled));
                cx.refresh_windows();
            },
            pal,
        ));

    #[cfg(target_os = "macos")]
    let rows = rows.child(settings_switch_row(
        "settings-menu-bar",
        tr!("Show in menu bar"),
        tr!("Keep OpenLogi's icon in the menu bar. When off, it stays in the Dock instead."),
        show_in_menu_bar,
        |enabled, cx| {
            cx.update_global::<AppState, _>(move |s, _| s.set_show_in_menu_bar(enabled));
            cx.refresh_windows();
        },
        pal,
    ));

    settings_page_shell(
        tr!("General"),
        tr!("Basic OpenLogi behavior and startup preferences."),
        pal,
        settings_card(rows, pal),
    )
}

fn embedded_permissions_settings(pal: Palette, cx: &mut App) -> impl IntoElement {
    let accessibility = if cx
        .try_global::<AppState>()
        .is_some_and(|s| s.accessibility_granted)
    {
        PermissionStatus::Granted
    } else {
        PermissionStatus::Denied
    };

    let rows = v_flex()
        .gap_0()
        .child(settings_action_row(
            tr!("Accessibility"),
            tr!("Needed for gesture and button remapping (event tap)."),
            permission_field(
                "main-perm-accessibility",
                accessibility,
                Permission::Accessibility,
                pal,
            ),
            pal,
        ))
        .child(settings_action_row(
            tr!("Input Monitoring"),
            tr!("Needed to read HID++ data, including Bluetooth-direct mice."),
            permission_field(
                "main-perm-input-monitoring",
                permissions::input_monitoring(),
                Permission::InputMonitoring,
                pal,
            ),
            pal,
        ))
        .child(settings_action_row(
            tr!("Bluetooth"),
            tr!("Allows OpenLogi to use CoreBluetooth (not required for HID access)."),
            permission_field(
                "main-perm-bluetooth",
                permissions::bluetooth(),
                Permission::Bluetooth,
                pal,
            ),
            pal,
        ));

    settings_page_shell(
        tr!("Permissions"),
        tr!("macOS privacy permissions used by OpenLogi."),
        pal,
        settings_card(rows, pal),
    )
}

fn embedded_language_settings(
    language_select: Entity<SelectState<Vec<LanguageOption>>>,
    pal: Palette,
) -> impl IntoElement {
    settings_page_shell(
        tr!("Language"),
        tr!("Choose the interface language."),
        pal,
        settings_card(
            settings_action_row(
                tr!("Language"),
                tr!("Choose the interface language."),
                language_select_field(language_select),
                pal,
            ),
            pal,
        ),
    )
}

fn settings_page_shell(
    title: SharedString,
    description: SharedString,
    pal: Palette,
    content: impl IntoElement,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .overflow_y_scrollbar()
        .p_6()
        .child(
            v_flex()
                .w_full()
                .max_w(px(720.))
                .gap_4()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xl()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(pal.text_muted)
                                .child(description),
                        ),
                )
                .child(content),
        )
}

fn settings_card(content: impl IntoElement, pal: Palette) -> impl IntoElement {
    div()
        .w_full()
        .rounded_lg()
        .border_1()
        .border_color(pal.border)
        .bg(pal.surface)
        .child(content)
}

fn settings_switch_row(
    id: &'static str,
    title: SharedString,
    description: SharedString,
    checked: bool,
    on_toggle: impl Fn(bool, &mut App) + 'static,
    pal: Palette,
) -> impl IntoElement {
    settings_action_row(
        title,
        description,
        Switch::new(id)
            .checked(checked)
            .on_click(move |enabled, _, cx| {
                on_toggle(*enabled, cx);
            }),
        pal,
    )
}

fn settings_action_row(
    title: SharedString,
    description: SharedString,
    control: impl IntoElement,
    pal: Palette,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .gap_4()
        .px_4()
        .py_3()
        .border_b_1()
        .border_color(pal.border)
        .child(
            v_flex()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(pal.text_muted)
                        .child(description),
                ),
        )
        .child(control)
}
