//! Platform and OS integration helpers.

pub mod branding;
pub mod launch_agent;
pub mod permissions;
pub mod single_instance;
#[cfg(target_os = "macos")]
mod status_item;
#[cfg(target_os = "macos")]
mod tray_row;
pub mod tray;
pub mod updater;
