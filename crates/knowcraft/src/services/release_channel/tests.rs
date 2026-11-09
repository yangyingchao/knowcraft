// SPDX-License-Identifier: MIT

use super::Version;

fn parse(tag: &str) -> Version {
    Version::parse(tag).unwrap_or_else(|| panic!("expected {tag} to parse"))
}

#[test]
fn staged_prereleases_order_within_same_core() {
    assert!(parse("0.5.0-alpha.2") > parse("0.5.0-alpha.1"));
    assert!(parse("0.5.0-beta.2") > parse("0.5.0-beta.1"));
    assert!(parse("0.5.0-rc.2") > parse("0.5.0-rc.1"));
    assert!(parse("0.5.0-alpha.9") > parse("0.5.0-nightly.20260901"));
    assert!(parse("0.5.0-beta.1") > parse("0.5.0-alpha.9"));
    assert!(parse("0.5.0-rc.1") > parse("0.5.0-beta.9"));
}

#[test]
fn final_release_outranks_its_own_prerelease() {
    assert!(parse("0.5.0") > parse("0.5.0-rc.2"));
}

#[test]
fn patch_bump_outranks_prior_release() {
    assert!(parse("0.5.0") > parse("0.4.9"));
}

#[test]
fn nightly_ordinal_orders_by_date_within_same_core() {
    assert!(parse("0.5.0-nightly.20260901") > parse("0.5.0-nightly.20260831"));
}

#[test]
fn nightly_same_date_orders_by_suffix() {
    assert!(parse("0.5.0-nightly.20260901.2") > parse("0.5.0-nightly.20260901.1"));
    assert!(parse("0.5.0-nightly.20260901.1") > parse("0.5.0-nightly.20260901"));
}

#[test]
fn equal_versions_compare_equal() {
    assert_eq!(parse("0.5.0-rc.1"), parse("0.5.0-rc.1"));
    assert_eq!(parse("0.5.0"), parse("0.5.0"));
    assert_eq!(
        parse("0.5.0-nightly.20260901.2"),
        parse("0.5.0-nightly.20260901.2")
    );
}

#[test]
fn rc_outranks_nightly_for_the_same_core() {
    // Pinned by D5: within an equal core, a release candidate compares
    // greater than a nightly build. This matches semver §11's alphanumeric
    // comparison of prerelease identifiers ("nightly" sorts before "rc"),
    // and reflects that once an RC is cut for a core version, that line
    // has stabilized -- a same-core nightly should never outrank it.
    assert!(parse("0.5.0-rc.1") > parse("0.5.0-nightly.20260901"));
}

#[test]
fn nightly_suffix_does_not_collide_with_the_next_days_date() {
    // Regression test: the ordinal must not be a packed `date * 1000 + n`
    // integer, since the grammar places no bound on `N`. A large suffix
    // must never spill into the date component.
    assert!(parse("0.5.0-nightly.20260901.1000") != parse("0.5.0-nightly.20260902"));
    assert!(parse("0.5.0-nightly.20260901.1000") < parse("0.5.0-nightly.20260902"));
}

#[test]
fn accepts_canonical_forms() {
    assert!(Version::parse("0.0.0").is_some());
    assert!(Version::parse("v0.5.0").is_some());
    assert!(Version::parse("0.5.0").is_some());
    assert!(Version::parse("v0.5.0-alpha.1").is_some());
    assert!(Version::parse("v0.5.0-beta.1").is_some());
    assert!(Version::parse("v0.5.0-rc.1").is_some());
    assert!(Version::parse("v0.5.0-nightly.20260901").is_some());
    assert!(Version::parse("v0.5.0-nightly.20260901.2").is_some());
}

/// Missing or malformed segments must be rejected outright. The old
/// `parse_version` silently zero-filled them, which was the bug behind
/// issue #61: a truncated tag could compare as a real version.
#[test]
fn rejects_malformed_tags() {
    assert!(Version::parse("0.5").is_none());
    assert!(Version::parse("0.5.x").is_none());
    assert!(Version::parse("v0.5.0-preview.1").is_none());
    assert!(Version::parse("v0.5.0-alpha.0").is_none());
    assert!(Version::parse("v0.5.0-beta.0").is_none());
    assert!(Version::parse("v0.5.0-rc.0").is_none());
    assert!(Version::parse("v0.5.0-rc").is_none());
    assert!(Version::parse("v0.5.0-rc.x").is_none());
    assert!(Version::parse("").is_none());
    assert!(Version::parse("vv0.5.0").is_none());
    assert!(Version::parse("0.5.0-rc.1-extra").is_none());
}

#[test]
fn display_renders_canonical_tags() {
    for (input, expected) in [
        ("v0.5.0-alpha.1", "0.5.0-alpha.1"),
        ("v0.5.0-beta.2", "0.5.0-beta.2"),
        ("v0.5.0-rc.3", "0.5.0-rc.3"),
        ("v0.5.0", "0.5.0"),
        ("v0.5.0-nightly.20260901.2", "0.5.0-nightly.20260901.2"),
        ("v0.5.0-nightly.20260901", "0.5.0-nightly.20260901"),
        (
            "v0.5.0-nightly.20260901.1000",
            "0.5.0-nightly.20260901.1000",
        ),
    ] {
        assert_eq!(parse(input).to_string(), expected);
    }
}
