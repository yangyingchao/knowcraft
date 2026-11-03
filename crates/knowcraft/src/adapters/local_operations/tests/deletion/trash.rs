// SPDX-License-Identifier: MIT

use super::*;

fn delete_ended(event: &OperationEvent) -> bool {
    matches!(
        event,
        OperationEvent::Deleted { .. } | OperationEvent::CompletedWithErrors { .. }
    )
}

/// Deletes `entries` as request `id` and waits until it ends, returning
/// the operation with what it emitted.
fn delete_and_wait(
    id: u64,
    entries: Vec<FileEntry>,
    permanent: bool,
) -> (LoadHandle, Vec<OperationEvent>) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let operation = LocalOperationProvider.delete(
        DeleteRequest {
            id: OperationRequestId(id),
            entries,
            permanent,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    wait_for_operation(&events, delete_ended);
    (operation, events.take())
}

/// Restores `location` from Trash and waits until it ends.
fn restore_and_wait(location: Location) -> (LoadHandle, Vec<OperationEvent>) {
    let restoring = Rc::new(RefCell::new(Vec::new()));
    let emitted = restoring.clone();
    let operation = LocalOperationProvider.restore(
        RestoreRequest {
            id: OperationRequestId(96),
            source: RestoreSource::OriginalLocations {
                locations: vec![location],
                originals: HashMap::new(),
            },
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    wait_for_operation(&restoring, |event| {
        matches!(
            event,
            OperationEvent::Restored { .. }
                | OperationEvent::RestoreCompletedWithErrors { .. }
                | OperationEvent::Failed { .. }
        )
    });
    (operation, restoring.take())
}

fn restored_items(events: &[OperationEvent]) -> Vec<(Location, ItemIdentity)> {
    events
        .iter()
        .filter_map(|event| match event {
            OperationEvent::ItemRestored {
                location, identity, ..
            } => Some((location.clone(), *identity)),
            _ => None,
        })
        .collect()
}

fn check_identity_reports(permanent: bool) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let folder = root.path().join("folder");
    fs::create_dir(&folder)?;
    let identity = numbers_at(&folder);
    let (_operation, events) = delete_and_wait(95, vec![directory_entry(&folder)], permanent);

    assert!(matches!(
        events.last(),
        Some(OperationEvent::Deleted { .. })
    ));
    let expected = if permanent {
        Vec::new()
    } else {
        vec![(Location::local(&folder), identity)]
    };
    assert_eq!(trashed_items(&events), expected);
    let trashed = events.iter().find_map(|event| match event {
        OperationEvent::ItemTrashed { identity, .. } => Some(*identity),
        _ => None,
    });
    let Some(trashed) = trashed else {
        return Ok(());
    };
    let (_restoring, restoring) = restore_and_wait(Location::local(&folder));
    let restored = restored_items(&restoring);
    assert!(
        matches!(&restored[..], [(location, restored)]
            if *location == Location::local(&folder) && restored.unchanged(&trashed)),
        "{restored:?} after {:?}",
        restoring.last()
    );
    Ok(())
}

#[test]
fn trashing_and_restoring_report_each_items_identity_and_a_permanent_delete_reports_none() {
    crate::test_support::gtk_test(
        "adapters::local_operations::tests::deletion::trash::trashing_and_restoring_report_each_items_identity_and_a_permanent_delete_reports_none",
        || {
            for permanent in [false, true] {
                check_identity_reports(permanent).expect("identity reports");
            }
        },
    );
}

#[derive(Clone, Copy, Debug)]
enum Swap {
    Untouched,
    RenamedOver,
    MovedAside,
}

impl Swap {
    /// Puts `arrival` at `item`'s name as this says.
    fn racer(self, arrival: PathBuf, item: PathBuf, aside: PathBuf) -> Racer {
        Box::new(move || match self {
            Swap::Untouched => {}
            Swap::RenamedOver => fs::rename(&arrival, &item).expect("renamed over"),
            Swap::MovedAside => {
                fs::rename(&item, &aside).expect("moved aside");
                fs::rename(&arrival, &item).expect("renamed in");
            }
        })
    }
}

fn check_trashed_identity(
    (preset, op, swap, reports_arrival): (Preset, Op, Swap, bool),
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let item = root.path().join("report.txt");
    let arrival = root.path().join("arrival.txt");
    fs::write(&item, b"checked")?;
    fs::write(&arrival, b"arrived later")?;
    let (checked, arrived) = (numbers_at(&item), numbers_at(&arrival));
    let racer = swap.racer(arrival, item.clone(), root.path().join("report.txt~"));
    let _quirks = FsQuirks::install(
        root.path(),
        Quirks {
            before: vec![(op, "report.txt", 0, racer)],
            ..preset()
        },
    );
    let (_operation, events) = delete_and_wait(13, vec![file_entry(&item)], false);

    assert!(
        matches!(events.last(), Some(OperationEvent::Deleted { .. })),
        "{op:?}, {swap:?}: {:?}",
        events.last()
    );
    assert_eq!(
        trashed_items(&events),
        [(
            Location::local(&item),
            if reports_arrival { arrived } else { checked }
        )],
        "{:?}, {op:?}, {swap:?}",
        preset().mount_type
    );
    Ok(())
}

#[test]
fn trash_reports_the_item_it_moved_when_another_took_the_name_first() {
    crate::test_support::gtk_test(
        "adapters::local_operations::tests::deletion::trash::trash_reports_the_item_it_moved_when_another_took_the_name_first",
        || {
            let rows: [(Preset, Op, Swap, bool); 5] = [
                (Quirks::ext4, Op::Trash, Swap::Untouched, false),
                (Quirks::ext4, Op::Trash, Swap::RenamedOver, true),
                (Quirks::ext4, Op::OpenAt, Swap::RenamedOver, true),
                (Quirks::sshfs, Op::Trash, Swap::MovedAside, true),
                (Quirks::nfs3, Op::Trash, Swap::RenamedOver, false),
            ];
            for row in rows {
                check_trashed_identity(row).expect("trashed identity");
            }
        },
    );
}

fn check_trash_moving_nothing(
    (trash, preset, evicted): (Trash, Preset, Option<bool>),
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let folder = root.path().join("folder");
    fs::create_dir(&folder)?;
    let evicting = Arc::new(Mutex::new(None));
    let (path, outcome) = (folder.clone(), evicting.clone());
    let racer: Racer = Box::new(move || {
        *outcome.lock().expect("outcome") = Some(super::super::fs_quirks::evict(&path));
    });
    let _quirks = FsQuirks::install(
        root.path(),
        Quirks {
            trash,
            after: if evicted.is_some() {
                vec![(Op::Trash, "folder", 0, racer)]
            } else {
                Vec::new()
            },
            ..preset()
        },
    );
    let (_operation, events) = delete_and_wait(4060, vec![directory_entry(&folder)], false);

    assert!(
        matches!(
            events.last(),
            Some(OperationEvent::CompletedWithErrors { deleted_locations, retryable_locations, .. })
                if deleted_locations.is_empty()
                    && retryable_locations == &[Location::local(&folder)]
        ),
        "{trash:?}: {:?}",
        events.last()
    );
    assert!(trashed_items(&events).is_empty());
    assert!(folder.is_dir());
    assert_eq!(
        *evicting.lock().map_err(|_| "outcome")?,
        evicted,
        "{:?}",
        preset().mount_type
    );
    Ok(())
}

#[test]
fn a_trash_that_moves_nothing_is_retryable_and_records_no_undo() {
    crate::test_support::gtk_test(
        "adapters::local_operations::tests::deletion::trash::a_trash_that_moves_nothing_is_retryable_and_records_no_undo",
        || {
            let rows: [(Trash, Preset, Option<bool>); 5] = [
                (Trash::FalseWithoutError, Quirks::default, None),
                (Trash::SucceedsWithoutMoving, Quirks::default, None),
                (Trash::SucceedsWithoutMoving, Quirks::vfat, Some(false)),
                (Trash::SucceedsWithoutMoving, Quirks::sshfs, Some(false)),
                (Trash::SucceedsWithoutMoving, Quirks::exfat, Some(false)),
            ];
            for row in rows {
                check_trash_moving_nothing(row).expect("a trash that moves nothing");
            }
        },
    );
}
