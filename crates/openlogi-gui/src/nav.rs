//! Main-window sidebar selection (devices + settings sections).

/// What the main window's detail pane is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarNav {
    Devices,
    Device(usize),
    General,
    Permissions,
    Language,
}

impl SidebarNav {
    #[must_use]
    pub fn settings_section(self) -> Option<crate::settings_pages::SettingsSection> {
        match self {
            Self::General => Some(crate::settings_pages::SettingsSection::General),
            Self::Permissions => Some(crate::settings_pages::SettingsSection::Permissions),
            Self::Language => Some(crate::settings_pages::SettingsSection::Language),
            Self::Devices | Self::Device(_) => None,
        }
    }
}
