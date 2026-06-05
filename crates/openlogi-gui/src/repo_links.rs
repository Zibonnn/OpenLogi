//! Outbound GitHub links for the About window and application menu.
//!
//! `CARGO_PKG_REPOSITORY` comes from the workspace `repository` field in
//! `Cargo.toml` — change it there when the fork moves.

pub const REPO_URL: &str = env!("CARGO_PKG_REPOSITORY");
pub const HELP_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "#readme");
pub const RELEASES_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases/latest");
/// Release page for the running build, linked from the About version label.
pub const RELEASE_TAG_URL: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/releases/tag/v",
    env!("CARGO_PKG_VERSION")
);
