//! Colors and shared sizes for the OpenLogi UI.
//!
//! Two layers:
//!
//! - **Brand / status** colours are fixed `u32` constants. They're saturated
//!   enough to read on both light and dark backgrounds, so they don't change
//!   with the OS appearance (the OpenLogi accent blue, the connectivity dots).
//! - **Surface / text** colours flip with the appearance and live in
//!   [`Palette`], chosen by [`palette`] from the active gpui-component theme
//!   mode. The bespoke surfaces (window, cards, mouse model)
//!   read these so they track the same light/dark switch as gpui-component's
//!   own widgets — which is what keeps a popover from rendering white under
//!   an otherwise dark UI (see `main.rs`'s appearance wiring).

use gpui::{App, Hsla};
use gpui_component::ActiveTheme as _;

/// Primary action / selection blue. Brand colour, identical in both modes —
/// it reads on the light card surfaces and the dark window alike.
pub const ACCENT_BLUE: u32 = 0x003b_82f6;

/// Status colours for the connectivity dot.
pub const STATUS_CONNECTED: u32 = 0x0022_c55e;
pub const STATUS_CONNECTING: u32 = 0x00ea_b308;
pub const STATUS_OFFLINE: u32 = 0x006b_7280;

/// Sizes that several components need to agree on.
pub const FOOTER_H: f32 = 36.;

/// Device gallery card width.
pub const GALLERY_CARD_W: f32 = 240.;

/// Device gallery image area height.
pub const GALLERY_PHOTO_H: f32 = 170.;

/// Appearance-dependent colours resolved from the active gpui-component theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Main content background.
    pub bg: Hsla,
    /// Raised card / panel fill.
    pub surface: Hsla,
    /// Row hover fill.
    pub surface_hover: Hsla,
    /// Content-area borders.
    pub border: Hsla,
    /// Primary text.
    pub text_primary: Hsla,
    /// Muted metadata.
    pub text_muted: Hsla,
}

/// Resolve colours from gpui-component's theme so app surfaces track light/dark mode.
#[must_use]
pub fn palette(cx: &App) -> Palette {
    let c = cx.theme().colors;
    Palette {
        bg: c.background,
        surface: c.secondary,
        surface_hover: c.list_hover,
        border: c.border,
        text_primary: c.foreground,
        text_muted: c.muted_foreground,
    }
}
