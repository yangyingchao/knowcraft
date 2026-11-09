// SPDX-License-Identifier: MIT

//! Pure channel and version types for the self-updater.
//!
//! This module is the single place in the codebase that interprets release
//! tags and reasons about their precedence. It performs no I/O and knows
//! nothing about GitHub's API shapes -- callers hand it plain strings and
//! get back structured, comparable values.

use std::{cmp::Ordering, fmt};

/// What a given release build IS, independent of the user's channel
/// preference.
///
/// Kept separate from [`BuildKind`] because each prerelease stage retains a
/// distinct user-facing label even when several are accepted by one channel.
///
/// Declaration order pins precedence for prereleases sharing a core version
/// (see [`Version`]'s `Ord` impl): `Nightly < Alpha < Beta < Rc`. A final
/// release always outranks every prerelease of its core version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BuildKind {
    Stable,
    Nightly,
    Alpha,
    Beta,
    Rc,
}

impl BuildKind {
    /// The build kind's name in the app language, for display.
    pub(crate) fn localized_label(self) -> String {
        crate::i18n::tr(match self {
            BuildKind::Stable => "Stable",
            BuildKind::Nightly => "build_kind.nightly",
            BuildKind::Alpha => "Alpha",
            BuildKind::Beta => "Beta",
            BuildKind::Rc => "Release candidate",
        })
    }
}

/// A comparable ordinal for a prerelease build.
///
/// For an RC, `primary` is the candidate number and `suffix` is always
/// zero. For a nightly, `primary` is the `YYYYMMDD` date and `suffix` is
/// the optional `.N` disambiguator (zero when absent).
///
/// These are kept as two separate fields, ordered lexicographically,
/// rather than packed into a single integer. Packing (e.g. `date * 1000 +
/// n`) would let an unbounded `.N` spill into the date component -- the
/// grammar places no bound on `N` -- silently corrupting both the ordering
/// and the round-tripped `Display` output. Two fields cannot collide this
/// way.
///
/// Comparing an RC's `primary` against a nightly's `primary` would be
/// comparing a small integer against a date, which is meaningless; see
/// [`Version`]'s `Ord` impl, which only ever compares two `Ordinal`s after
/// confirming both sides share the same [`BuildKind`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Ordinal {
    primary: u64,
    suffix: u64,
}

/// A parsed prerelease suffix: its kind, an orderable ordinal, and the
/// original text as published.
#[derive(Clone, Debug)]
pub struct Prerelease {
    kind: BuildKind,
    ordinal: Ordinal,
}

/// A semver-correct, comparable representation of a release tag.
///
/// [`Version::parse`] is the *only* place release tags are interpreted
/// anywhere in the codebase. It accepts an optional leading `v` and exactly
/// three grammar forms:
///
/// ```text
/// v?MAJOR.MINOR.PATCH
/// v?MAJOR.MINOR.PATCH-alpha.N
/// v?MAJOR.MINOR.PATCH-beta.N
/// v?MAJOR.MINOR.PATCH-rc.N
/// v?MAJOR.MINOR.PATCH-nightly.YYYYMMDD[.N]
/// ```
///
/// Anything else returns `None` -- there is no silent zero-fill fallback.
#[derive(Clone, Debug)]
pub struct Version {
    core: (u64, u64, u64),
    prerelease: Option<Prerelease>,
}

/// Parses a string as a strictly non-negative, non-signed decimal `u64`.
///
/// `str::parse` alone is not strict enough here: it accepts a leading `+`,
/// which would let a malformed tag segment slip through.
fn parse_strict_u64(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn parse_core(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let major = parse_strict_u64(parts.next()?)?;
    let minor = parse_strict_u64(parts.next()?)?;
    let patch = parse_strict_u64(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn parse_prerelease(suffix: &str) -> Option<Prerelease> {
    let mut parts = suffix.split('.');
    match parts.next()? {
        kind @ ("alpha" | "beta" | "rc") => {
            let n = parse_strict_u64(parts.next()?)?;
            if n == 0 || parts.next().is_some() {
                return None;
            }
            let kind = match kind {
                "alpha" => BuildKind::Alpha,
                "beta" => BuildKind::Beta,
                _ => BuildKind::Rc,
            };
            Some(Prerelease {
                kind,
                ordinal: Ordinal {
                    primary: n,
                    suffix: 0,
                },
            })
        }
        "nightly" => {
            let date_str = parts.next()?;
            if date_str.len() != 8 {
                return None;
            }
            let date = parse_strict_u64(date_str)?;
            let suffix_n = match parts.next() {
                Some(n_str) => parse_strict_u64(n_str)?,
                None => 0,
            };
            if parts.next().is_some() {
                return None;
            }
            Some(Prerelease {
                kind: BuildKind::Nightly,
                ordinal: Ordinal {
                    primary: date,
                    suffix: suffix_n,
                },
            })
        }
        _ => None,
    }
}

impl Version {
    /// Parses a release tag per the grammar documented on [`Version`].
    /// Returns `None` for anything that does not match exactly.
    pub fn parse(tag: &str) -> Option<Version> {
        let rest = tag.strip_prefix('v').unwrap_or(tag);
        if rest.is_empty() {
            return None;
        }
        let (core_str, prerelease_str) = match rest.split_once('-') {
            Some((core, suffix)) => (core, Some(suffix)),
            None => (rest, None),
        };
        let core = parse_core(core_str)?;
        let prerelease = match prerelease_str {
            Some(suffix) => Some(parse_prerelease(suffix)?),
            None => None,
        };
        Some(Version { core, prerelease })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (major, minor, patch) = self.core;
        write!(f, "{major}.{minor}.{patch}")?;
        match &self.prerelease {
            Some(prerelease) => match prerelease.kind {
                BuildKind::Alpha => write!(f, "-alpha.{}", prerelease.ordinal.primary),
                BuildKind::Beta => write!(f, "-beta.{}", prerelease.ordinal.primary),
                BuildKind::Rc => write!(f, "-rc.{}", prerelease.ordinal.primary),
                BuildKind::Nightly => {
                    let date = prerelease.ordinal.primary;
                    let suffix_n = prerelease.ordinal.suffix;
                    if suffix_n == 0 {
                        write!(f, "-nightly.{date}")
                    } else {
                        write!(f, "-nightly.{date}.{suffix_n}")
                    }
                }
                BuildKind::Stable => Ok(()),
            },
            None => Ok(()),
        }
    }
}

impl Ord for Version {
    /// Per semver §11: compare core triples first; for equal cores, a
    /// prerelease is always less than a final release; for two prereleases
    /// on an equal core, compare `kind` first and *only then* `ordinal`.
    ///
    /// `kind` must be compared first: an RC's ordinal (a small candidate
    /// number) and a nightly's ordinal (a date) are different quantities
    /// and are never meaningfully comparable. Short-circuiting on `kind`
    /// via `then_with` guarantees `ordinal.cmp` only ever runs when both
    /// sides share the same [`BuildKind`].
    fn cmp(&self, other: &Self) -> Ordering {
        self.core
            .cmp(&other.core)
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.kind.cmp(&b.kind).then_with(|| a.ordinal.cmp(&b.ordinal)),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

#[cfg(test)]
mod tests;
