// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn package_metadata_is_available_to_the_about_page() {
    assert_eq!(REPOSITORY, "https://github.com/yangyingchao/knowcraft");
    assert_eq!(AUTHOR, "LGSE Ltd.");
}

/// The default `RELEASE_TAG` injected by `build.rs` (no `KNOWCRAFT_RELEASE_TAG`
/// override, i.e. this test run) is derived from a git tag and must never
/// carry the `git describe` short hash.
#[test]
fn default_release_tag_omits_the_short_hash() {
    assert!(
        RELEASE_TAG.starts_with('v'),
        "expected a v-prefixed tag, got {RELEASE_TAG:?}"
    );
    assert!(
        !RELEASE_TAG.contains("-g"),
        "short hash leaked into {RELEASE_TAG:?}"
    );
}

/// A tagged build parses to `VERSION`; an untagged build (`v0.1.1-N`) does
/// not match the release-channel grammar, so callers that need a comparable
/// version fall back to the `0.0.0` floor rather than panicking.
#[test]
fn release_tag_matches_the_core_version_when_tagged() {
    if let Some(parsed) = crate::services::Version::parse(RELEASE_TAG) {
        assert_eq!(parsed.to_string(), VERSION);
    }
}

/// A developer build has no `KNOWCRAFT_BUILD_KIND` override, so it must
/// report itself as `Stable` -- the fallback that keeps a build unable to
/// identify itself from silently claiming to be a preview.
#[test]
fn developer_build_reports_stable_build_kind() {
    assert_eq!(build_kind(), BuildKind::Stable);
}
