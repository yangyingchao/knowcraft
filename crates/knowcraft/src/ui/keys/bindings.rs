// SPDX-License-Identifier: MIT

//! Every shortcut as a named constant, grouped by area. Handlers match these
//! constants, and the F1 reference, hints and labels show the same ones.

use super::{Binding, Command, Digits};

/// Declares an area's constants and lists them for the registry and the
/// catalog tests. The F1 reference shows `listed` constants and leaves
/// `unlisted` ones out.
///
/// A `Command` names its id, and `structural` for a key read through aliases.
/// `= binding` binds one key and `=> [first, second]` alternates, primary
/// first. A single command that follows another names it with `after`.
macro_rules! catalog {
    (@after) => { None };
    (@after $prefix:path) => { Some($prefix) };

    (@kind) => { $crate::ui::keys::command::Kind::Single };
    (@kind structural) => { $crate::ui::keys::command::Kind::Structural };

    (@define [$($attr:tt)*] $name:ident Binding [$($ignored:tt)*] = $value:expr) => {
        $($attr)* pub(crate) const $name: Binding = $value;
    };
    (@define [$($attr:tt)*] $name:ident Digits [$($ignored:tt)*] = $value:expr) => {
        $($attr)* pub(crate) const $name: Digits = $value;
    };
    (
        @define [$($attr:tt)*] $name:ident Command [$id:literal $($kind:ident $(($group:ident))?)?]
            = $value:expr
    ) => {
        $($attr)* pub(crate) const $name: Command =
            Command::new(&$crate::ui::keys::command::Spec {
                id: $id,
                kind: catalog!(@kind $($kind $(($group))?)?),
                default: &[$value],
            });
    };
    (
        @define [$($attr:tt)*] $name:ident Command [$id:literal $($kind:ident $(($group:ident))?)?]
            => $alternates:tt
    ) => {
        $($attr)* pub(crate) const $name: Command =
            Command::new(&$crate::ui::keys::command::Spec {
                id: $id,
                kind: catalog!(@kind $($kind $(($group))?)?),
                default: &$alternates,
            });
    };

    (
        $(listed {
            $($(#[$meta:meta])* $name:ident: $kind:ident
                $($id:literal $($ck:ident $(($cg:ident))?)?)?
                $(= $value:expr)?
                $(=> $alternates:tt)?
                $(, after $prefix:path)?;)*
        })?
        $(unlisted {
            $($(#[$umeta:meta])* $uname:ident: $ukind:ident
                $($uid:literal $($uck:ident $(($ucg:ident))?)?)?
                $(= $uvalue:expr)?
                $(=> $ualternates:tt)?;)*
        })?
    ) => {
        $($(catalog!(@define [$(#[$meta])*] $name $kind [$($id $($ck $(($cg))?)?)?]
            $(= $value)? $(=> $alternates)?);)*)?
        $($(catalog!(@define [$(#[$umeta])*] $uname $ukind [$($uid $($uck $(($ucg))?)?)?]
            $(= $uvalue)? $(=> $ualternates)?);)*)?

        pub(super) const ENTRIES: &[super::Entry] = &[
            $($(super::Entry::$kind($name),)*)?
            $($(super::Entry::$ukind($uname),)*)?
        ];

        #[cfg(test)]
        pub(super) const LISTED: &[super::Listed] = &[
            $($(super::Listed {
                name: stringify!($name),
                entry: super::Entry::$kind($name),
                after: catalog!(@after $($prefix)?),
            },)*)?
        ];
        #[cfg(test)]
        pub(super) const UNLISTED: &[(&str, super::Entry)] =
            &[$($((stringify!($uname), super::Entry::$ukind($uname)),)*)?];
    };
}

mod chooser;
mod dialogs;
mod navigation;
mod preview;
mod reference;
mod widgets;
mod window;

pub(crate) use chooser::*;
pub(crate) use dialogs::*;
pub(crate) use navigation::*;
pub(crate) use preview::*;
pub(crate) use reference::*;
pub(crate) use widgets::*;
pub(crate) use window::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Entry {
    Binding(Binding),
    Digits(Digits),
    Command(Command),
}

/// Every constant the catalog declares, listed or not.
pub(super) fn entries() -> impl Iterator<Item = Entry> {
    [
        chooser::ENTRIES,
        dialogs::ENTRIES,
        navigation::ENTRIES,
        preview::ENTRIES,
        reference::ENTRIES,
        widgets::ENTRIES,
        window::ENTRIES,
    ]
    .into_iter()
    .flatten()
    .copied()
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(super) struct Listed {
    pub(super) name: &'static str,
    pub(super) entry: Entry,
    /// The command this one follows, besides a chord step's trigger.
    pub(super) after: Option<Command>,
}

#[cfg(test)]
pub(super) fn listed() -> impl Iterator<Item = Listed> {
    [
        chooser::LISTED,
        dialogs::LISTED,
        navigation::LISTED,
        preview::LISTED,
        reference::LISTED,
        widgets::LISTED,
        window::LISTED,
    ]
    .into_iter()
    .flatten()
    .copied()
}

#[cfg(test)]
pub(super) fn unlisted() -> impl Iterator<Item = (&'static str, Entry)> {
    [
        chooser::UNLISTED,
        dialogs::UNLISTED,
        navigation::UNLISTED,
        preview::UNLISTED,
        reference::UNLISTED,
        widgets::UNLISTED,
        window::UNLISTED,
    ]
    .into_iter()
    .flatten()
    .copied()
}
