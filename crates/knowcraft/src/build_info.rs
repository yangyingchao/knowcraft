// SPDX-License-Identifier: MIT

use crate::services::BuildKind;

pub const VERSION: &str = knowcraft_core::VERSION;
pub const COMMIT: &str = env!("KNOWCRAFT_BUILD_COMMIT");
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
pub const AUTHOR: &str = env!("CARGO_PKG_AUTHORS");

/// The release tag this build was published under, e.g. `v0.5.0` or
/// `v0.5.0-rc.1`.
///
/// Injected by `build.rs`, mirroring how it injects [`COMMIT`]. Derived from
/// the git tag; untagged commits carry a commit-distance suffix like
/// `v0.5.0-3`, and a checkout without tags falls back to `v0.0.0`.
/// `Cargo.toml` keeps a permanent placeholder version and is not a version
/// source. Prerelease identity still travels here without bumping manifest
/// metadata (see D3 in the release-channel design notes).
///
/// Only the build-info tests read it today; the display path uses the core
/// [`VERSION`] string so it matches the CLI exactly.
#[cfg(test)]
pub const RELEASE_TAG: &str = knowcraft_core::RELEASE_TAG;

/// The build kind this binary was published as, as the raw string
/// `build.rs` injected: `"stable"`, `"alpha"`, `"beta"`, `"rc"`, or
/// `"nightly"`.
///
/// Defaults to `"stable"` when `KNOWCRAFT_BUILD_KIND` is unset, empty, or not
/// one of those three values. Prefer [`build_kind`] over reading this
/// directly -- it gives the parsed, comparable [`BuildKind`].
pub const BUILD_KIND: &str = env!("KNOWCRAFT_BUILD_KIND");

/// Parses [`BUILD_KIND`], falling back to [`BuildKind::Stable`] for
/// anything unrecognised.
///
/// This must fail closed, the same way [`BuildKind`]'s parser
/// does: a build that cannot identify its own kind must present as an
/// ordinary stable build, never as a preview.
pub fn build_kind() -> BuildKind {
    match BUILD_KIND {
        "nightly" => BuildKind::Nightly,
        "alpha" => BuildKind::Alpha,
        "beta" => BuildKind::Beta,
        "rc" => BuildKind::Rc,
        _ => BuildKind::Stable,
    }
}

/// The version string the About page, diagnostics and `--version` show.
///
/// Identical to the CLI's: both read `knowcraft_core::VERSION` verbatim, so a
/// build between tags shows the commit-distance suffix too (e.g. `0.1.2-3`).
pub fn displayed_version() -> String {
    VERSION.to_owned()
}

#[cfg(test)]
mod tests;
