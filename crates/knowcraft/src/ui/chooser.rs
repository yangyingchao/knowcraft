// SPDX-License-Identifier: MIT

mod destination;
mod download;
mod geometry;
mod image_conversion;
mod layout;
mod options;
mod shortcuts;
mod source;
mod state;

#[cfg(test)]
mod tests;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::OsStr,
    path::{Component, Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::TryRecvError,
    },
    time::Duration,
};

use ashpd::{
    PortalError, WindowIdentifierType,
    desktop::file_chooser::{Choice, FileFilter, SelectedFiles},
};
use gtk::{gdk::ModifierType, gio, glib, prelude::*};

use crate::{
    adapters::{LocalFileSource, LocalOperationProvider, LocalPreviewProvider, probes},
    app::BrowserEvent,
    model::{EntryKind, FileEntry, Location, MetadataValue},
    portal::{
        ChooserKind, ChooserRequest, local_uri, open_selection, safe_filename,
        writable_from_read_only,
    },
    services::{
        DirectoryChange, DirectoryEvent, DirectoryRequest, FileSource, ItemIdentity, LoadHandle,
        LocationValidationError, MetadataRequest, RemoteDownload, StampAnswer, WatchRequest,
        Watching, download_remote, remote_file_url,
    },
};

use super::{
    blur::BlurBin,
    browser::{BrowserView, dismiss_modal_layer, modal_layer},
    browser_modes::{BrowserMode, ThumbnailResize},
    controls::{
        ModalTone, focus_button, form_check_button, form_entry, form_label, menu_option,
        message_dialog_layout,
    },
    marquee::OriginClick,
    modal::close_on_escape,
    preferences::PreferenceManager,
    preview::{PreviewDrawer, preview_target},
    shortcut_footer::ShortcutFooter,
    top_bar_navigation::TopBarNavigation,
    window::{
        ChooserKeys, ChooserPolicy, MIN_SIDEBAR_WIDTH, SIDEBAR_WIDTH, SidebarView,
        build_appearance_menu, build_sidebar, edit_focused_text, home_folder_location,
        install_modal_focus_trap, visible_modal_layer,
    },
};
use super::{
    focus_navigation::{
        FOCUS_MODIFIERS, arrow_direction, plain_tab_direction, vim_focus_direction,
    },
    keys::{self, KeyPress, Map, SHORTCUT_MODIFIERS, bindings::*},
};

use destination::DestinationHost;
pub(crate) use destination::{DestinationRequest, present_destination_chooser};
use geometry::{chooser_initial_dimensions, detect_monitor_geometry};
use layout::build_chooser_hosted;
use options::{ChoiceControl, ChooserDropdown, PortalFilter};
use source::ChooserFileSource;
#[cfg(test)]
use source::OUTSIDE_ROOT;
use state::ChooserState;

type Completion = Box<dyn FnOnce(ashpd::backend::Result<SelectedFiles>)>;
type DestinationValidator =
    Rc<dyn Fn(&Path) -> futures_lite::future::BoxedLocal<Result<PathBuf, String>>>;

thread_local! {
    static CHOOSERS: RefCell<HashMap<String, glib::WeakRef<gtk::Window>>> = RefCell::new(HashMap::new());
    static DESTINATION_PARENTS: RefCell<Vec<glib::WeakRef<gtk::Window>>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
thread_local! {
    static DESTINATION_STATES: RefCell<Vec<std::rc::Weak<ChooserState>>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) fn destination_chooser_for(parent: &gtk::Window) -> Option<(gtk::Window, BrowserView)> {
    DESTINATION_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .filter_map(std::rc::Weak::upgrade)
            .find(|state| {
                state.window.is_visible() && state.window.transient_for().as_ref() == Some(parent)
            })
            .map(|state| (state.window.clone(), state.view.clone()))
    })
}

/// Opens the file chooser for a portal request from another application.
pub(crate) fn present_portal_request(
    request: ChooserRequest,
    completion: impl FnOnce(ashpd::backend::Result<SelectedFiles>) + 'static,
) {
    crate::ensure_assets();
    let directory_only = matches!(
        &request.kind,
        ChooserKind::Open {
            directory: true,
            ..
        }
    );
    let source = Rc::new(ChooserFileSource {
        root_limit: None,
        source: Rc::new(LocalFileSource),
        filter: Rc::new(RefCell::new(None)),
        directory_only: Rc::new(Cell::new(directory_only)),
    });
    let cancelled = Arc::new(AtomicBool::new(false));
    glib::MainContext::default().spawn_local(async move {
        crate::portal::prepare_chooser_placement().await;
        build_chooser_hosted(request, cancelled, completion, source, None);
    });
}
