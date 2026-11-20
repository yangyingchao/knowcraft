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
