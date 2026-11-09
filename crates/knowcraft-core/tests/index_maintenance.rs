use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use knowcraft_core::index::Index;
use knowcraft_core::index_maintenance::{IndexMaintenance, IndexRequest, IndexScope};
use knowcraft_core::model::Document;
use knowcraft_core::store::Store;
use knowcraft_core::write_lock::WriteLock;

struct Fixture {
    dir: PathBuf,
    store: Store,
    index: Index,
    maintenance: IndexMaintenance,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let dir =
            std::env::temp_dir().join(format!("kc-index-maint-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inbox")).unwrap();
        let store = Store::open(&dir).unwrap();
        let index = Index::open(&dir).unwrap();
        let lock = Arc::new(WriteLock::new(&dir));
        let maintenance = IndexMaintenance::new(store.clone(), index.clone(), dir.clone(), lock);
        Fixture {
            dir,
            store,
            index,
            maintenance,
        }
    }

    fn with_threads(mut self, threads: usize) -> Fixture {
        self.maintenance = IndexMaintenance::new(
            self.store.clone(),
            self.index.clone(),
            self.dir.clone(),
            Arc::new(WriteLock::new(&self.dir)),
        )
        .with_threads(threads);
        self
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.dir.join("inbox").join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    fn register(&self, path: &Path) -> i64 {
        let doc = Document {
            id: 0,
            uuid: format!("uuid-{}", path.display()),
            storage: "linked".to_string(),
            original_name: String::new(),
            path: path.to_string_lossy().into_owned(),
            doc_type: "md".to_string(),
            title: path.file_name().unwrap().to_string_lossy().into_owned(),
            source_url: None,
            size: 0,
            created_at: String::new(),
            updated_at: String::new(),
            checksum: String::new(),
            ai_summary: None,
            lifecycle: "pending".to_string(),
            index_status: "pending".to_string(),
        };
        self.store.insert_document(&doc).unwrap()
    }

    fn run(&self, request: IndexRequest) -> knowcraft_core::index_maintenance::IndexReport {
        self.maintenance.run(request).unwrap()
    }

    fn incremental_all(&self) -> knowcraft_core::index_maintenance::IndexReport {
        self.run(IndexRequest::Incremental {
            scope: IndexScope::All,
        })
    }

    fn hits(&self, word: &str) -> Vec<i64> {
        self.index
            .search(word, 10)
            .unwrap()
            .into_iter()
            .map(|hit| hit.document_id)
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn file_fingerprint(path: &Path) -> (Option<i64>, i64) {
    let metadata = std::fs::metadata(path).unwrap();
    let mtime = metadata
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;
    (Some(mtime), metadata.len() as i64)
}

#[test]
fn incremental_indexes_once_then_reports_unchanged() {
    let fixture = Fixture::new("unchanged");
    let path = fixture.write("a.md", "alpha body");
    let id = fixture.register(&path);

    let first = fixture.incremental_all();
    assert_eq!(first.indexed, 1);
    assert_eq!(first.unchanged, 0);
    assert_eq!(fixture.hits("alpha"), vec![id]);

    let state = fixture.store.list_index_states().unwrap();
    let recorded = state.get(&id).expect("index state recorded");
    assert_eq!(
        recorded,
        &knowcraft_core::store::IndexState {
            indexed_mtime: file_fingerprint(&path).0,
            indexed_size: Some(file_fingerprint(&path).1),
        }
    );

    let second = fixture.incremental_all();
    assert_eq!(second.indexed, 0);
    assert_eq!(second.unchanged, 1);
    assert!(second.skipped_unregistered == 0 && second.failed == 0);
}

#[test]
fn changed_file_is_reindexed() {
    let fixture = Fixture::new("changed");
    let path = fixture.write("a.md", "alpha body");
    let id = fixture.register(&path);
    fixture.incremental_all();

    std::thread::sleep(Duration::from_millis(5));
    std::fs::write(&path, "beta replacement").unwrap();
    let report = fixture.incremental_all();
    assert_eq!(report.indexed, 1);
    assert_eq!(report.unchanged, 0);
    assert_eq!(fixture.hits("beta"), vec![id]);
    assert!(fixture.hits("alpha").is_empty());
}

#[test]
fn same_size_rewrite_is_detected_by_mtime() {
    let fixture = Fixture::new("mtime");
    let path = fixture.write("a.md", "alpha body");
    let _id = fixture.register(&path);
    fixture.incremental_all();

    std::thread::sleep(Duration::from_millis(5));
    std::fs::write(&path, "beta  body").unwrap();
    let report = fixture.incremental_all();
    assert_eq!(report.indexed, 1, "相同大小改写应通过 mtime 检出");
}

#[test]
fn missing_file_is_counted_and_removed_from_index() {
    let fixture = Fixture::new("missing");
    let path = fixture.write("a.md", "alpha body");
    let id = fixture.register(&path);
    fixture.incremental_all();

    std::fs::remove_file(&path).unwrap();
    let report = fixture.incremental_all();
    assert_eq!(report.missing, 1);
    assert_eq!(report.indexed, 0);
    assert!(fixture.hits("alpha").is_empty());
    assert_eq!(
        fixture
            .store
            .get_document(id)
            .unwrap()
            .unwrap()
            .index_status,
        "error"
    );
}

#[test]
fn trashed_document_is_removed_as_orphan() {
    let fixture = Fixture::new("trashed");
    let path = fixture.write("a.md", "alpha body");
    let id = fixture.register(&path);
    fixture.incremental_all();
    fixture.store.set_lifecycle(id, "trashed").unwrap();

    let report = fixture.incremental_all();
    assert_eq!(report.indexed, 0);
    assert_eq!(report.unchanged, 0);
    assert_eq!(report.orphans_removed, 1);
    assert!(fixture.hits("alpha").is_empty());
}

#[test]
fn hidden_documents_never_enter_the_index() {
    let fixture = Fixture::new("hidden");
    let path = fixture.write(".secret/h.md", "hidden needle");
    let _id = fixture.register(&path);

    let report = fixture.run(IndexRequest::Rebuild);
    assert_eq!(report.indexed, 0);
    assert!(fixture.hits("hidden").is_empty());
}

#[test]
fn deleted_document_leaves_no_orphan_entry() {
    let fixture = Fixture::new("orphan");
    let path = fixture.write("a.md", "alpha body");
    let id = fixture.register(&path);
    fixture.incremental_all();
    fixture.store.delete_document(id).unwrap();

    let report = fixture.incremental_all();
    assert_eq!(report.orphans_removed, 1);
    assert!(fixture.hits("alpha").is_empty());
}

#[test]
fn path_scope_limits_rebuild_and_counts_unregistered_files() {
    let fixture = Fixture::new("scope");
    let a = fixture.write("a.md", "alpha body");
    let b = fixture.write("b.md", "beta body");
    let a_id = fixture.register(&a);
    let b_id = fixture.register(&b);
    fixture.incremental_all();
    assert_eq!(fixture.hits("alpha"), vec![a_id]);
    assert_eq!(fixture.hits("beta"), vec![b_id]);

    fixture.write("unregistered.md", "ignored body");
    std::fs::write(&a, "alpha changed").unwrap();
    std::fs::write(&b, "beta changed").unwrap();
    let report = fixture.run(IndexRequest::Incremental {
        scope: IndexScope::Paths(vec![fixture.dir.join("inbox")]),
    });
    assert_eq!(report.indexed, 2);
    assert_eq!(report.skipped_unregistered, 1);
}

#[test]
fn single_file_scope_leaves_siblings_untouched() {
    let fixture = Fixture::new("single-scope");
    let a = fixture.write("a.md", "alpha body");
    let b = fixture.write("b.md", "beta body");
    let a_id = fixture.register(&a);
    let b_id = fixture.register(&b);
    fixture.incremental_all();

    std::fs::write(&a, "alpha changed").unwrap();
    std::fs::write(&b, "beta changed").unwrap();
    let report = fixture.run(IndexRequest::Incremental {
        scope: IndexScope::Paths(vec![a.clone()]),
    });
    assert_eq!(report.indexed, 1);
    assert_eq!(report.unchanged, 0);
    assert_eq!(fixture.hits("alpha"), vec![a_id]);
    assert_eq!(fixture.hits("beta"), vec![b_id]);
}

#[test]
fn parallel_extraction_matches_single_threaded() {
    let build = |name: &str, threads: usize| {
        let fixture = Fixture::new(name).with_threads(threads);
        for file in ["a.md", "b.md", "c.md", "d.md"] {
            let path = fixture.write(file, &format!("{file} alpha beta gamma"));
            fixture.register(&path);
        }
        fixture
    };
    let single = build("parallel-single", 1);
    let report = single.incremental_all();
    let mut single_hits: Vec<i64> = single.hits("alpha");
    single_hits.sort_unstable();

    let parallel = build("parallel-many", 4);
    let parallel_report = parallel.incremental_all();
    let mut parallel_hits: Vec<i64> = parallel.hits("alpha");
    parallel_hits.sort_unstable();

    assert_eq!(report, parallel_report);
    assert_eq!(report.indexed, 4);
    assert_eq!(single_hits, parallel_hits);
}

#[test]
fn rebuild_discards_stale_entries() {
    let fixture = Fixture::new("rebuild");
    let path = fixture.write("a.md", "alpha body");
    let id = fixture.register(&path);

    let stale_path = fixture.write("stale.md", "stale content");
    let stale = Document {
        id: 999,
        uuid: "stale-uuid".to_string(),
        storage: "linked".to_string(),
        original_name: String::new(),
        path: stale_path.to_string_lossy().into_owned(),
        doc_type: "md".to_string(),
        title: "stale".to_string(),
        source_url: None,
        size: 0,
        created_at: String::new(),
        updated_at: String::new(),
        checksum: String::new(),
        ai_summary: None,
        lifecycle: "pending".to_string(),
        index_status: "pending".to_string(),
    };
    fixture.index.index_document(&stale).unwrap();

    let report = fixture.run(IndexRequest::Rebuild);
    assert_eq!(report.indexed, 1);
    assert_eq!(fixture.hits("alpha"), vec![id]);
    assert!(fixture.hits("stale").is_empty());
}

#[test]
fn rebuild_reports_documents_whose_files_are_gone() {
    let fixture = Fixture::new("rebuild-missing");
    let missing = fixture.dir.join("inbox/gone.md");
    let id = fixture.register(&missing);

    let report = fixture.run(IndexRequest::Rebuild);
    assert_eq!(report.missing, 1);
    assert_eq!(report.indexed, 0);
    assert_eq!(
        fixture
            .store
            .get_document(id)
            .unwrap()
            .unwrap()
            .index_status,
        "error"
    );
    assert!(fixture.index.entries().unwrap().is_empty());
}
