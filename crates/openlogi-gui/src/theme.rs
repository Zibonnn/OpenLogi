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

use gpui::{App, BoxShadow, Hsla, hsla, point, px};
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
    /// Raised card / panel fill (used by component library).
    pub surface: Hsla,
    /// Row hover fill.
    pub surface_hover: Hsla,
    /// Content-area borders.
    pub border: Hsla,
    /// Primary text.
    pub text_primary: Hsla,
    /// Muted metadata.
    pub text_muted: Hsla,
    /// Sidebar surface — #f5f5f5 in light mode, gpui-component `sidebar` in dark.
    pub sidebar_bg: Hsla,
    /// Elevated card surface (white in light mode, raised dark in dark mode).
    pub card_bg: Hsla,
    /// Card background on hover — identical to card_bg in light (shadow does
    /// the work), slightly lighter than card_bg in dark (adds visible lift).
    pub card_hover_bg: Hsla,
    /// Muted-gray canvas drawn behind cards.
    pub window_bg: Hsla,
    /// Hairline separator between sidebar and content pane.
    pub sidebar_border: Hsla,
}

/// Resolve colours from gpui-component's theme so app surfaces track light/dark mode.
#[must_use]
pub fn palette(cx: &App) -> Palette {
    let c = cx.theme().colors;
    let is_light = c.background.l > 0.5;

    // macOS native dark grouped-background is ~#1C1C1E (hsl 240, 3.5%, 11.4%).
    // The gpui-component dark theme has a near-black #131313 which reads as
    // pure black on screen. Override with the proper macOS dark canvas so cards
    // stand out against the pane background.
    let dark_canvas = hsla(240. / 360., 0.035, 0.114, 1.0);
    let light_canvas = hsla(0., 0., 0.98, 1.0); // #fafafa — neutral-50

    // Card surface: slightly elevated above the canvas.
    // Light → pure background (white); dark → secondary (raised dark surface).
    let card = if is_light { c.background } else { c.secondary };

    // Hover bg: in light mode the shadow lift is sufficient so keep the same
    // white; in dark mode bump lightness by ~4% for a perceptible lift.
    let card_hover = if is_light {
        card
    } else {
        Hsla { l: (card.l + 0.04).min(1.0), ..card }
    };

    Palette {
        surface: c.secondary,
        surface_hover: c.list_hover,
        border: c.border,
        text_primary: c.foreground,
        text_muted: c.muted_foreground,
        sidebar_bg: if is_light { c.muted } else { c.sidebar },
        card_bg: card,
        card_hover_bg: card_hover,
        window_bg: if is_light { light_canvas } else { dark_canvas },
        sidebar_border: Hsla {
            a: if is_light { 0.09 } else { 0.18 },
            ..c.border
        },
    }
}

/// Soft two-layer shadow for elevated card surfaces (Craft-style).
#[must_use]
pub fn card_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla(0., 0., 0., 0.07),
            offset: point(px(0.), px(1.)),
            blur_radius: px(4.),
            spread_radius: px(0.),
        },
        BoxShadow {
            color: hsla(0., 0., 0., 0.05),
            offset: point(px(0.), px(4.)),
            blur_radius: px(16.),
            spread_radius: px(0.),
        },
    ]
}

/// Stronger shadow for card hover / active states — same surface, more lift.
#[must_use]
pub fn card_shadow_hover() -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla(0., 0., 0., 0.10),
            offset: point(px(0.), px(2.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
        },
        BoxShadow {
            color: hsla(0., 0., 0., 0.08),
            offset: point(px(0.), px(8.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
        },
    ]
}
