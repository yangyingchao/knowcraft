use std::path::{Path, PathBuf};
use std::sync::Arc;

use knowcraft_core::fileops::{DeleteOutcome, FileOps, Imported, Touched};
use knowcraft_core::index::Index;
use knowcraft_core::model::Document;
use knowcraft_core::store::Store;
use knowcraft_core::write_lock::WriteLock;

struct Fixture {
    dir: PathBuf,
    store: Store,
    index: Index,
    ops: FileOps,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("kc-fileops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir).unwrap();
        let index = Index::open(&dir).unwrap();
        let lock = Arc::new(WriteLock::new(&dir));
        let ops = FileOps::new(store.clone(), index.clone(), dir.clone(), lock).unwrap();
        Fixture {
            dir,
            store,
            index,
            ops,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn doc_at(fixture: &Fixture, path: &Path) -> Option<Document> {
    fixture.store.find_by_path(&path.to_string_lossy()).unwrap()
}

fn search_has(fixture: &Fixture, word: &str, id: i64) -> bool {
    fixture
        .index
        .search(word, 10)
        .unwrap()
        .iter()
        .any(|hit| hit.document_id == id)
}

fn outside_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kc-fileops-out-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 先用 `create` 建立空文件并登记，再写入内容并按 `record_copy` 刷新记录与索引。
fn create_with_content(fixture: &Fixture, path: &Path, content: &str) -> Document {
    let doc = fixture
        .ops
        .create(path, false)
        .unwrap()
        .expect("file created");
    write_file(path, content);
    let report = fixture.ops.record_copy(&[path.to_path_buf()]).unwrap();
    assert_eq!(report.files, 1);
    fixture.store.get_document(doc.id).unwrap().unwrap()
}

#[test]
fn rejects_paths_outside_the_data_directory() {
    let fixture = Fixture::new("outside");
    let outside = outside_dir("outside");
    let file = outside.join("loot.md");
    write_file(&file, "outside");

    let dst = fixture.path("inbox/loot.md");
    assert!(
        fixture.ops.create(&file, false).is_err(),
        "库外路径不能创建"
    );
    assert!(
        fixture.ops.copy(&file, &dst).is_err(),
        "库外路径不能作为复制源"
    );
    assert!(
        fixture.ops.move_path(&file, &dst).is_err(),
        "库外路径不能作为移动源"
    );
    assert!(fixture.ops.delete(&file).is_err(), "库外路径不能删除");
    assert!(
        fixture.ops.create(Path::new("relative.md"), false).is_err(),
        "相对路径被拒绝"
    );

    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn rejects_data_dir_and_managed_roots() {
    let fixture = Fixture::new("roots");
    let inbox_file = fixture.path("inbox/f.md");
    write_file(&inbox_file, "x");

    assert!(
        fixture.ops.create(&fixture.dir, false).is_err(),
        "data_dir 本身被拒绝"
    );
    assert!(
        fixture.ops.delete(&fixture.dir).is_err(),
        "data_dir 本身被拒绝"
    );
    for root in [
        "inbox", "library", "notes", "captures", "archived", ".trash", ".tmp",
    ] {
        let root = fixture.path(root);
        assert!(
            fixture.ops.create(&root, true).is_err(),
            "受管/内部根目录不能作为操作对象: {}",
            root.display()
        );
        assert!(
            fixture.ops.delete(&root).is_err(),
            "受管/内部根目录不能作为删除对象: {}",
            root.display()
        );
    }
}

#[test]
fn rejects_symbolic_links() {
    let fixture = Fixture::new("symlink");
    let outside = outside_dir("symlink");
    let target = outside.join("secret.md");
    write_file(&target, "secret");

    let link = fixture.path("inbox/link.md");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(fixture.ops.delete(&link).is_err(), "叶节点符号链接被拒绝");
    assert!(
        fixture
            .ops
            .move_path(&link, &fixture.path("library/link.md"))
            .is_err(),
        "符号链接不能移动"
    );

    let link_dir = fixture.path("inbox/alias");
    std::os::unix::fs::symlink(&outside, &link_dir).unwrap();
    assert!(
        fixture
            .ops
            .create(&link_dir.join("created.md"), false)
            .is_err(),
        "位于符号链接目录内的路径被拒绝"
    );

    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn rejects_destination_inside_source() {
    let fixture = Fixture::new("nested");
    let src = fixture.path("inbox/tree");
    std::fs::create_dir_all(&src).unwrap();
    write_file(&src.join("a.md"), "a");

    assert!(
        fixture.ops.copy(&src, &src.join("inner")).is_err(),
        "目标位于源之内被拒绝"
    );
    assert!(
        fixture.ops.move_path(&src, &src.join("inner")).is_err(),
        "目标位于源之内被拒绝"
    );
    assert!(fixture.ops.copy(&src, &src).is_err(), "目标与源相同被拒绝");

    let dst = fixture.path("library/tree");
    assert!(fixture.ops.copy(&src, &dst).is_ok(), "正常复制可用作对照");
}

#[test]
fn create_registers_files_by_location_and_ignores_directories() {
    let fixture = Fixture::new("create");
    let inbox = fixture.path("inbox/alpha.md");
    let doc = create_with_content(&fixture, &inbox, "# Alpha\nalphaword");
    assert_eq!(doc.lifecycle, "pending");
    assert_eq!(doc.storage, "linked");
    assert_eq!(doc.index_status, "indexed");
    assert_eq!(doc.path, inbox.to_string_lossy());
    assert!(search_has(&fixture, "alphaword", doc.id));

    let folder = fixture.path("inbox/folder");
    assert!(
        fixture.ops.create(&folder, true).unwrap().is_none(),
        "目录不登记"
    );
    assert!(fixture
        .store
        .find_by_path(&folder.to_string_lossy())
        .unwrap()
        .is_none());

    let library = fixture.path("library/beta.md");
    let lib_doc = fixture.ops.create(&library, false).unwrap().unwrap();
    assert_eq!(lib_doc.lifecycle, "archived");
    assert_eq!(lib_doc.storage, "linked");

    let archived = fixture.path("archived/md/gamma.md");
    std::fs::create_dir_all(archived.parent().unwrap()).unwrap();
    let arc_doc = fixture.ops.create(&archived, false).unwrap().unwrap();
    assert_eq!(arc_doc.lifecycle, "archived");
    assert_eq!(arc_doc.storage, "archived");

    let no_parent = fixture.path("inbox/missing/never.md");
    assert!(
        fixture.ops.create(&no_parent, false).is_err(),
        "父目录不存在时创建失败"
    );
}

#[test]
fn create_rejects_existing_targets_and_allows_empty_duplicates() {
    let fixture = Fixture::new("create-dup");
    let existing = fixture.path("inbox/exists.md");
    write_file(&existing, "existing");
    assert!(
        fixture.ops.create(&existing, false).is_err(),
        "同名文件已存在时失败"
    );
    assert_eq!(
        std::fs::read(&existing).unwrap(),
        b"existing",
        "不覆盖既有内容"
    );

    let existing_dir = fixture.path("inbox/exists-dir");
    std::fs::create_dir_all(&existing_dir).unwrap();
    assert!(
        fixture.ops.create(&existing_dir, true).is_err(),
        "同名目录已存在时失败"
    );

    let first = fixture.path("inbox/empty-a.md");
    let second = fixture.path("inbox/empty-b.md");
    assert!(fixture.ops.create(&first, false).unwrap().is_some());
    assert!(
        fixture.ops.create(&second, false).unwrap().is_some(),
        "空文件按路径为身份，可并存"
    );
}

#[test]
fn touch_creates_registers_and_skips_registered_files() {
    let fixture = Fixture::new("touch");
    let created = fixture.path("inbox/touched.md");
    let Touched::Created(doc) = fixture.ops.touch(&created).unwrap() else {
        panic!("新文件应返回 Created");
    };
    assert_eq!(doc.lifecycle, "pending");
    assert_eq!(doc.index_status, "indexed");
    assert_eq!(doc.path, created.to_string_lossy());

    assert!(
        matches!(fixture.ops.touch(&created).unwrap(), Touched::Skipped),
        "已登记时幂等跳过"
    );

    let existing = fixture.path("notes/existing.md");
    write_file(&existing, "existingword");
    let Touched::Registered(doc) = fixture.ops.touch(&existing).unwrap() else {
        panic!("未登记文件应返回 Registered");
    };
    assert_eq!(doc.lifecycle, "archived");
    assert!(search_has(&fixture, "existingword", doc.id));

    let folder = fixture.path("inbox/folder");
    std::fs::create_dir_all(&folder).unwrap();
    assert!(fixture.ops.touch(&folder).is_err(), "目录不能 touch");

    let no_parent = fixture.path("inbox/missing/never.md");
    assert!(fixture.ops.touch(&no_parent).is_err(), "父目录不存在时失败");
}

#[test]
fn delete_all_handles_multiple_paths_and_permanent_mode() {
    let fixture = Fixture::new("delete-all");
    let first = fixture.path("inbox/one.md");
    let second = fixture.path("inbox/two.md");
    write_file(&first, "delete one");
    write_file(&second, "delete two");
    let missing = fixture.path("inbox/missing.md");

    let report = fixture
        .ops
        .delete_all(&[first.clone(), missing.clone(), second.clone()], false)
        .unwrap();
    assert_eq!(report.removed.len(), 2);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].path, missing);
    assert!(!first.exists());
    assert!(!second.exists());
    assert!(matches!(
        report.removed[0].outcome,
        DeleteOutcome::Trashed(_)
    ));

    let removed = fixture.path("library/removed.md");
    write_file(&removed, "remove permanently");
    let report = fixture.ops.delete_all(&[removed.clone()], true).unwrap();
    assert_eq!(report.removed.len(), 1);
    assert!(matches!(
        report.removed[0].outcome,
        DeleteOutcome::Purged(_)
    ));
    assert!(!removed.exists());
    assert!(
        fixture
            .store
            .find_by_path(&removed.to_string_lossy())
            .unwrap()
            .is_none(),
        "彻底删除同时移除记录"
    );
}

#[test]
fn copy_gives_registered_file_a_new_identity() {
    let fixture = Fixture::new("copy-file");
    let src = fixture.path("inbox/source.md");
    let doc = create_with_content(&fixture, &src, "# Source\ncopyalpha body");

    let dst = fixture.path("inbox/copied.md");
    let report = fixture.ops.copy(&src, &dst).unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(report.registered, 1);
    assert_eq!(report.index_failed, 0);

    let copied = doc_at(&fixture, &dst).expect("副本已登记");
    assert_ne!(copied.uuid, doc.uuid, "副本使用新 uuid");
    assert_eq!(copied.lifecycle, "pending");
    assert_eq!(copied.index_status, "indexed");
    assert_eq!(std::fs::read(&dst).unwrap(), std::fs::read(&src).unwrap());
    assert!(search_has(&fixture, "copyalpha", copied.id));
}

#[test]
fn copy_registers_unregistered_source() {
    let fixture = Fixture::new("copy-loos");
    let src = fixture.path("inbox/loose.md");
    write_file(&src, "looseword");
    assert!(doc_at(&fixture, &src).is_none(), "源文件未登记");

    let dst = fixture.path("library/loose.md");
    let report = fixture.ops.copy(&src, &dst).unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(report.registered, 1);

    let doc = doc_at(&fixture, &dst).expect("未登记源复制后也登记");
    assert_eq!(doc.lifecycle, "archived");
    assert_eq!(doc.storage, "linked");
    assert_eq!(doc.index_status, "indexed");
}

#[test]
fn copy_directory_recurses_and_skips_hidden_entries() {
    let fixture = Fixture::new("copy-dir");
    let src = fixture.path("inbox/tree");
    write_file(&src.join("a.md"), "treea");
    write_file(&src.join("nested/b.md"), "treeb");
    write_file(&src.join(".hidden.md"), "treehidden");

    let dst = fixture.path("library/tree");
    let report = fixture.ops.copy(&src, &dst).unwrap();
    assert_eq!(report.files, 2, "隐藏文件不计入");
    assert_eq!(report.registered, 2);

    assert!(dst.join("a.md").exists());
    assert!(dst.join("nested/b.md").exists());
    assert!(!dst.join(".hidden.md").exists(), "隐藏文件不被复制");
    assert!(doc_at(&fixture, &dst.join("a.md")).is_some());
    assert!(doc_at(&fixture, &dst.join("nested/b.md")).is_some());
}

#[test]
fn move_between_managed_directories_keeps_lifecycle() {
    let fixture = Fixture::new("move-managed");
    let src = fixture.path("inbox/mv.md");
    let doc = create_with_content(&fixture, &src, "# Move\nmoveword");

    let dst = fixture.path("library/cat/mv.md");
    let report = fixture.ops.move_path(&src, &dst).unwrap();
    assert_eq!(report.docs, 1);
    assert_eq!(report.index_failed, 0);

    assert!(!src.exists(), "源文件已移走");
    assert!(dst.exists(), "目标文件存在");
    let moved = doc_at(&fixture, &dst).expect("记录更新到新路径");
    assert_eq!(moved.id, doc.id, "受管目录间移动保留记录身份");
    assert_eq!(moved.lifecycle, "pending", "移动不执行归档语义");
    assert_eq!(moved.storage, "linked");
    assert_eq!(moved.original_name, "mv.md");
    assert!(search_has(&fixture, "moveword", doc.id));
}

#[test]
fn move_into_archived_sets_archived_state() {
    let fixture = Fixture::new("move-archived");
    let src = fixture.path("inbox/report.md");
    let doc = create_with_content(&fixture, &src, "# Report\narchword");

    let dst = fixture.path("library/report.md");
    fixture.ops.move_path(&src, &dst).unwrap();

    let target = fixture.path(&format!("archived/md/{}.md", doc.uuid));
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    let report = fixture.ops.move_path(&dst, &target).unwrap();
    assert_eq!(report.docs, 1);

    let moved = doc_at(&fixture, &target).expect("记录更新到 archived");
    assert_eq!(moved.id, doc.id);
    assert_eq!(moved.storage, "archived");
    assert_eq!(moved.lifecycle, "archived");
    assert_eq!(
        moved.original_name, "report.md",
        "archived 文件名不改写原名"
    );
    assert!(search_has(&fixture, "archword", doc.id));
}

#[test]
fn move_directory_rebases_all_descendants() {
    let fixture = Fixture::new("move-dir");
    let src = fixture.path("inbox/proj");
    let a = src.join("a.md");
    let b = src.join("nested/b.md");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    let doc_a = create_with_content(&fixture, &a, "proja");
    let doc_b = create_with_content(&fixture, &b, "projb");

    let dst = fixture.path("library/proj");
    let report = fixture.ops.move_path(&src, &dst).unwrap();
    assert_eq!(report.docs, 2);
    assert_eq!(report.index_failed, 0);

    assert!(!src.exists());
    let moved_a = doc_at(&fixture, &dst.join("a.md")).expect("后代 a 已更新");
    let moved_b = doc_at(&fixture, &dst.join("nested/b.md")).expect("后代 b 已更新");
    assert_eq!(moved_a.id, doc_a.id);
    assert_eq!(moved_b.id, doc_b.id);
    assert!(fixture
        .store
        .find_by_path(&a.to_string_lossy())
        .unwrap()
        .is_none());
    assert!(search_has(&fixture, "proja", doc_a.id));
    assert!(search_has(&fixture, "projb", doc_b.id));
}

#[test]
fn record_move_into_trash_keeps_record() {
    let fixture = Fixture::new("record-trash");
    let src = fixture.path("inbox/tm.md");
    write_file(&src, "tmword");
    let doc = fixture.ops.record_create(&src).unwrap().unwrap();

    let report = fixture
        .ops
        .record_move(&src, &fixture.path(".trash/files/tm.md"))
        .unwrap();
    assert_eq!(report.docs, 1);

    let stored = fixture.store.get_document(doc.id).unwrap().unwrap();
    assert_eq!(stored.lifecycle, "trashed");
    assert_eq!(
        stored.path,
        fixture.path(".trash/files/tm.md").to_string_lossy()
    );
    assert!(!search_has(&fixture, "tmword", doc.id), "索引条目被移出");
}

#[test]
fn delete_keeps_trashed_record_and_removes_index() {
    let fixture = Fixture::new("delete");
    let path = fixture.path("inbox/del.md");
    let doc = create_with_content(&fixture, &path, "# Del\ndelword");

    let report = fixture.ops.delete(&path).unwrap();
    assert_eq!(report.trashed, 1);

    let stored = fixture.store.get_document(doc.id).unwrap().unwrap();
    assert_eq!(stored.lifecycle, "trashed");
    assert!(
        stored.path.contains(".trash/files/"),
        "记录指向回收站落点: {}",
        stored.path
    );
    assert!(Path::new(&stored.path).exists());
    assert!(!path.exists(), "原文件已移走");
    assert!(!search_has(&fixture, "delword", doc.id), "索引移除");
    let info = fixture.path(".trash/info");
    assert_eq!(
        std::fs::read_dir(&info).unwrap().count(),
        1,
        "删除时写入 trashinfo"
    );
}

#[test]
fn delete_registers_unregistered_file_before_trashing() {
    let fixture = Fixture::new("delete-raw");
    let path = fixture.path("inbox/raw.md");
    write_file(&path, "rawword");
    assert!(doc_at(&fixture, &path).is_none());

    let report = fixture.ops.delete(&path).unwrap();
    assert_eq!(report.trashed, 1);

    let docs = fixture.store.list_documents().unwrap();
    assert_eq!(docs.len(), 1, "未登记文件先登记后删除");
    let stored = &docs[0];
    assert_eq!(stored.lifecycle, "trashed");
    assert!(Path::new(&stored.path).exists());
    assert!(!path.exists());
}

#[test]
fn delete_directory_trashes_every_regular_file() {
    let fixture = Fixture::new("delete-dir");
    let dir = fixture.path("inbox/tree");
    write_file(&dir.join("a.md"), "da");
    write_file(&dir.join("sub/b.md"), "db");
    write_file(&dir.join(".hidden.md"), "dc");

    let report = fixture.ops.delete(&dir).unwrap();
    assert_eq!(report.trashed, 3, "隐藏文件也计入");
    assert!(!dir.exists(), "目录被移除");

    let docs = fixture.store.list_documents().unwrap();
    assert_eq!(docs.len(), 3);
    assert!(docs.iter().all(|doc| doc.lifecycle == "trashed"));
    assert!(docs.iter().all(|doc| Path::new(&doc.path).exists()));
}

#[test]
fn restore_returns_to_original_path_and_shows_trailing_state() {
    let fixture = Fixture::new("restore");
    let path = fixture.path("inbox/restored.md");
    let doc = create_with_content(&fixture, &path, "# Restore\nrestoreword");
    fixture.ops.delete(&path).unwrap();

    let restored = fixture.ops.restore(&doc.uuid).unwrap();
    assert_eq!(restored.path, path.to_string_lossy());
    assert_eq!(restored.lifecycle, "pending");
    assert!(path.exists());
    assert_eq!(fixture.index.search("restoreword", 10).unwrap().len(), 1);
    assert_eq!(
        std::fs::read_dir(fixture.path(".trash/info"))
            .unwrap()
            .count(),
        0,
        "恢复后删除 trashinfo"
    );

    let other = fixture.path("inbox/other.md");
    let other_doc = create_with_content(&fixture, &other, "other");
    assert!(
        fixture.ops.restore(&other_doc.uuid).is_err(),
        "非回收站记录不能恢复"
    );
    assert!(
        fixture.ops.restore("no-such-uuid").is_err(),
        "不存在的 uuid 报错"
    );
}

#[test]
fn record_move_is_idempotent_and_tolerates_target_first() {
    let fixture = Fixture::new("record-move");
    let a = fixture.path("inbox/i1.md");
    write_file(&a, "idemword");
    let doc = fixture.ops.record_create(&a).unwrap().unwrap();

    let b = fixture.path("library/i1.md");
    std::fs::rename(&a, &b).unwrap();
    let first = fixture.ops.record_move(&a, &b).unwrap();
    assert_eq!(first.docs, 1);
    let second = fixture.ops.record_move(&a, &b).unwrap();
    assert_eq!(second.docs, 1, "重复调用只刷新既有记录");

    assert_eq!(
        fixture.store.list_documents().unwrap().len(),
        1,
        "不产生重复记录"
    );
    assert!(fixture
        .store
        .find_by_path(&a.to_string_lossy())
        .unwrap()
        .is_none());
    let stored = doc_at(&fixture, &b).unwrap();
    assert_eq!(stored.id, doc.id);
    assert!(search_has(&fixture, "idemword", doc.id));

    // 目标端先被登记（Created 事件先于 Moved）不触发 UNIQUE 冲突。
    let c = fixture.path("inbox/i2.md");
    write_file(&c, "secondword");
    let d = fixture.path("library/i2.md");
    std::fs::rename(&c, &d).unwrap();
    let created = fixture.ops.record_create(&d).unwrap().unwrap();
    let moved = fixture.ops.record_move(&c, &d).unwrap();
    assert_eq!(moved.docs, 1);
    assert_eq!(fixture.store.list_documents().unwrap().len(), 2);
    assert_eq!(doc_at(&fixture, &d).unwrap().id, created.id);
}

#[test]
fn record_copy_refreshes_registered_record() {
    let fixture = Fixture::new("record-copy");
    let path = fixture.path("inbox/refresh.md");
    write_file(&path, "first content");
    let doc = fixture.ops.record_create(&path).unwrap().unwrap();
    let before = doc.checksum.clone();

    write_file(&path, "second content with new size");
    let report = fixture
        .ops
        .record_copy(std::slice::from_ref(&path))
        .unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(report.registered, 0, "已登记记录只刷新");

    let refreshed = fixture.store.get_document(doc.id).unwrap().unwrap();
    assert_ne!(refreshed.checksum, before, "checksum 刷新");
    assert_eq!(
        refreshed.size,
        std::fs::metadata(&path).unwrap().len() as i64,
        "大小刷新"
    );
    assert_eq!(refreshed.index_status, "indexed");
    assert!(search_has(&fixture, "second", doc.id));

    let loose = fixture.path("inbox/loose.md");
    write_file(&loose, "looseword");
    let report = fixture
        .ops
        .record_copy(std::slice::from_ref(&loose))
        .unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(report.registered, 1, "未登记目标被登记");
    let doc = doc_at(&fixture, &loose).unwrap();
    assert_eq!(doc.lifecycle, "pending");
    assert!(search_has(&fixture, "looseword", doc.id));
}

#[test]
fn record_remove_deletes_records_but_spares_trashed() {
    let fixture = Fixture::new("record-remove");
    let path = fixture.path("inbox/rm.md");
    write_file(&path, "rmword");
    let doc = fixture.ops.record_create(&path).unwrap().unwrap();

    fixture.ops.record_remove(&path).unwrap();
    assert!(fixture.store.get_document(doc.id).unwrap().is_none());
    assert!(!search_has(&fixture, "rmword", doc.id));

    let kept = fixture.path("inbox/keep.md");
    let keep_doc = create_with_content(&fixture, &kept, "keepword");
    fixture.ops.delete(&kept).unwrap();
    let stored = fixture.store.get_document(keep_doc.id).unwrap().unwrap();
    fixture.ops.record_remove(Path::new(&stored.path)).unwrap();
    assert!(
        fixture.store.get_document(keep_doc.id).unwrap().is_some(),
        "trashed 记录保留"
    );
}

#[test]
fn import_indexes_files_and_directories() {
    let fixture = Fixture::new("import");
    let outside = outside_dir("import");

    let single = outside.join("single.md");
    write_file(&single, "importsingleword");
    match fixture.ops.import(&single).unwrap() {
        Imported::File(doc) => {
            assert!(doc.path.contains("/inbox/"));
            assert_eq!(doc.index_status, "indexed");
            assert!(search_has(&fixture, "importsingleword", doc.id));
        }
        other => panic!("单文件导入应返回文件，实际为 {other:?}"),
    }

    let folder = outside.join("batch");
    write_file(&folder.join("x.md"), "batchxword");
    write_file(&folder.join("nested/y.md"), "batchyword");
    match fixture.ops.import(&folder).unwrap() {
        Imported::Directory(report) => {
            assert_eq!(report.imported, 2);
            assert_eq!(report.updated, 0);
        }
        other => panic!("目录导入应返回目录，实际为 {other:?}"),
    }
    let x = fixture.path("inbox/batch/x.md");
    let y = fixture.path("inbox/batch/nested/y.md");
    let doc_x = doc_at(&fixture, &x).unwrap();
    let doc_y = doc_at(&fixture, &y).unwrap();
    assert_eq!(doc_x.index_status, "indexed");
    assert_eq!(doc_y.index_status, "indexed");
    assert!(search_has(&fixture, "batchxword", doc_x.id));
    assert!(search_has(&fixture, "batchyword", doc_y.id));

    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn import_all_bulk_sources_into_chosen_destination() {
    let fixture = Fixture::new("import-all");
    let outside = outside_dir("import-all");
    let single = outside.join("a.md");
    write_file(&single, "bulkaword");
    let folder = outside.join("papers");
    write_file(&folder.join("p1.md"), "bulkponeword");
    write_file(&folder.join("nested/p2.md"), "bulkptwoword");
    write_file(&folder.join(".hidden.md"), "hiddenword");

    let dest = fixture.path("library");
    let report = fixture
        .ops
        .import_all(&[single.clone(), folder.clone()], &dest, false)
        .unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(report.entries.len(), 2);

    let Imported::File(doc) = &report.entries[0].outcome else {
        panic!("单文件源应返回文件: {:?}", report.entries[0].outcome);
    };
    assert!(doc.path.ends_with("/library/a.md"), "{}", doc.path);
    assert_eq!(doc.lifecycle, "archived");
    assert_eq!(doc.index_status, "indexed");
    assert!(search_has(&fixture, "bulkaword", doc.id));

    let Imported::Directory(counts) = &report.entries[1].outcome else {
        panic!("目录源应返回目录: {:?}", report.entries[1].outcome);
    };
    assert_eq!(counts.imported, 2, "隐藏项不导入");
    assert_eq!(counts.updated, 0);
    assert!(fixture.path("library/papers/p1.md").is_file());
    assert!(fixture.path("library/papers/nested/p2.md").is_file());
    assert!(!fixture.path("library/papers/.hidden.md").exists());

    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn import_all_skips_existing_and_duplicate_until_forced() {
    let fixture = Fixture::new("import-skip");
    let outside = outside_dir("import-skip");
    let src = outside.join("note.md");
    write_file(&src, "skipv1word");
    let target = fixture.path("inbox/note.md");

    let Imported::File(first) = fixture.ops.import(&src).unwrap() else {
        panic!("首次导入应返回文件");
    };
    assert_eq!(target, Path::new(&first.path));

    let report = fixture
        .ops
        .import_all(&[src.clone()], &fixture.path("inbox"), false)
        .unwrap();
    assert!(matches!(
        report.entries[0].outcome,
        Imported::SkippedExisting
    ));

    let same = outside.join("copy.md");
    write_file(&same, "skipv1word");
    let report = fixture
        .ops
        .import_all(&[same.clone()], &fixture.path("inbox"), false)
        .unwrap();
    assert!(matches!(
        report.entries[0].outcome,
        Imported::SkippedDuplicate
    ));

    write_file(&src, "skipv2word");
    let report = fixture
        .ops
        .import_all(&[src.clone()], &fixture.path("inbox"), true)
        .unwrap();
    let Imported::Updated(updated) = &report.entries[0].outcome else {
        panic!("--force 应返回更新: {:?}", report.entries[0].outcome);
    };
    assert_eq!(updated.uuid, first.uuid, "覆盖保留原记录身份");
    assert_eq!(updated.id, first.id);
    assert_eq!(updated.index_status, "indexed");
    assert!(search_has(&fixture, "skipv2word", updated.id));
    assert!(!search_has(&fixture, "skipv1word", updated.id));

    // `--force` 仍按 checksum 跳过库内其它文档已有的内容。
    let duplicate = outside.join("other.md");
    write_file(&duplicate, "skipv2word");
    let report = fixture
        .ops
        .import_all(&[duplicate], &fixture.path("inbox"), true)
        .unwrap();
    assert!(matches!(
        report.entries[0].outcome,
        Imported::SkippedDuplicate
    ));

    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn import_all_rejects_library_sources_and_invalid_destinations() {
    let fixture = Fixture::new("import-reject");
    let inside = fixture.path("inbox/inside.md");
    write_file(&inside, "insideword");

    let report = fixture
        .ops
        .import_all(&[inside.clone()], &fixture.path("library"), false)
        .unwrap();
    assert!(report.entries.is_empty());
    assert_eq!(report.failed.len(), 1);
    assert!(
        report.failed[0]
            .error
            .contains("inside the managed library"),
        "{}",
        report.failed[0].error
    );

    let missing = fixture.path("library/missing-dest");
    let error = fixture
        .ops
        .import_all(&[inside.clone()], &missing, false)
        .unwrap_err();
    assert!(error.to_string().contains("not accessible"), "{error}");

    let internal = fixture.path(".tmp");
    let error = fixture
        .ops
        .import_all(&[inside], &internal, false)
        .unwrap_err();
    assert!(
        error.to_string().contains("not a managed location"),
        "{error}"
    );
}

#[test]
fn import_all_reports_structural_conflicts_per_source() {
    let fixture = Fixture::new("import-conflict");
    let outside = outside_dir("import-conflict");
    let folder = outside.join("blocked");
    write_file(&folder.join("a.md"), "blockedword");
    let blocker = fixture.path("library/blocked");
    std::fs::write(&blocker, "not a directory").unwrap();

    let report = fixture
        .ops
        .import_all(&[folder.clone()], &fixture.path("library"), false)
        .unwrap();
    assert!(report.entries.is_empty());
    assert_eq!(report.failed.len(), 1);
    assert!(
        report.failed[0].error.contains("not a directory"),
        "{}",
        report.failed[0].error
    );

    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn copy_and_move_create_missing_parent_directories() {
    let fixture = Fixture::new("parents");
    let src = fixture.path("inbox/p.md");
    write_file(&src, "parentword");

    let copy_dst = fixture.path("library/deep/sub/p.md");
    let report = fixture.ops.copy(&src, &copy_dst).unwrap();
    assert_eq!(report.files, 1);
    assert!(copy_dst.exists());
    assert!(doc_at(&fixture, &copy_dst).is_some());

    let move_dst = fixture.path("notes/other/sub/p.md");
    let src_doc = fixture.ops.record_create(&src).unwrap().unwrap();
    let report = fixture.ops.move_path(&src, &move_dst).unwrap();
    assert_eq!(report.docs, 1);
    assert!(move_dst.exists());
    let moved = doc_at(&fixture, &move_dst).unwrap();
    assert_eq!(moved.id, src_doc.id);
    assert!(!src.exists());
}

#[test]
fn rename_is_a_same_directory_move() {
    let fixture = Fixture::new("rename");
    let path = fixture.path("inbox/old.md");
    let doc = create_with_content(&fixture, &path, "# Rename\nrenameword");

    let report = fixture.ops.rename(&path, "new.md").unwrap();
    assert_eq!(report.docs, 1);
    let renamed = fixture.path("inbox/new.md");
    assert!(!path.exists());
    assert!(renamed.exists());
    let stored = doc_at(&fixture, &renamed).unwrap();
    assert_eq!(stored.id, doc.id);
    assert_eq!(stored.original_name, "new.md");
    assert!(
        fixture.ops.rename(&path, "a/b.md").is_err(),
        "名字含分隔符被拒绝"
    );
    assert!(fixture.ops.rename(&renamed, "").is_err(), "空名字被拒绝");
}

#[test]
fn register_registers_existing_file_in_place() {
    let fixture = Fixture::new("register-new");
    let path = fixture.path("notes/new.org");
    write_file(&path, "#+TITLE: 新笔记\n\nregisternewword");
    assert!(doc_at(&fixture, &path).is_none(), "文件尚未登记");

    let doc = fixture.ops.register(&path).unwrap();
    assert_eq!(doc.path, path.to_string_lossy(), "路径就地登记");
    assert_eq!(doc.lifecycle, "archived");
    assert_eq!(doc.storage, "linked");
    assert_eq!(doc.title, "新笔记", "标题取自内容");
    assert_eq!(doc.index_status, "indexed");
    assert!(path.exists(), "登记不复制、不移动文件");
    assert!(search_has(&fixture, "registernewword", doc.id));
}

#[test]
fn register_refreshes_registered_file_title_and_index() {
    let fixture = Fixture::new("register-refresh");
    let path = fixture.path("notes/refresh.org");
    write_file(&path, "#+TITLE: 旧标题\n\noldnewword");
    let before = fixture.ops.register(&path).unwrap();
    assert_eq!(before.title, "旧标题");
    let original_name = before.original_name.clone();

    write_file(&path, "#+TITLE: 新标题\n\nrefreshnewword");
    let after = fixture.ops.register(&path).unwrap();
    assert_eq!(after.id, before.id, "重复登记保留记录身份");
    assert_eq!(after.title, "新标题", "标题刷新");
    assert_eq!(after.lifecycle, before.lifecycle, "lifecycle 不变");
    assert_eq!(after.storage, before.storage, "storage 不变");
    assert_eq!(after.original_name, original_name, "original_name 不变");
    assert_eq!(after.path, before.path, "path 不变");
    assert_eq!(after.index_status, "indexed");
    assert!(search_has(&fixture, "refreshnewword", after.id));
    assert!(
        !search_has(&fixture, "oldnewword", after.id),
        "旧正文不再命中"
    );
}

#[test]
fn register_all_registers_each_file_in_place() {
    let fixture = Fixture::new("register-all");
    let first = fixture.path("notes/a.org");
    let second = fixture.path("library/b.org");
    write_file(&first, "#+TITLE: A\n\nbatchoneword");
    write_file(&second, "#+TITLE: B\n\nbatchtwoword");

    let report = fixture
        .ops
        .register_all(&[first.clone(), second.clone()])
        .unwrap();
    assert_eq!(report.registered.len(), 2);
    assert!(report.failed.is_empty());
    assert_eq!(report.registered[0].path, first.to_string_lossy());
    assert_eq!(report.registered[1].path, second.to_string_lossy());
    assert_eq!(
        report.registered[0].lifecycle, "archived",
        "位置决定生命周期"
    );
    assert!(search_has(
        &fixture,
        "batchoneword",
        report.registered[0].id
    ));
    assert!(search_has(
        &fixture,
        "batchtwoword",
        report.registered[1].id
    ));
}

#[test]
fn register_all_expands_directories_and_skips_hidden_entries() {
    let fixture = Fixture::new("register-all-dir");
    let root = fixture.path("notes/batch");
    let first = root.join("a.org");
    let second = root.join("nested/b.org");
    let hidden = root.join(".hidden.org");
    write_file(&first, "#+TITLE: A\n\nvisibleone");
    write_file(&second, "#+TITLE: B\n\nvisibletwo");
    write_file(&hidden, "#+TITLE: hidden\n\nhiddenword");

    let outside = outside_dir("register-all-dir");
    let outside_file = outside.join("out.org");
    write_file(&outside_file, "#+TITLE: outside\n\noutsideword");
    let link = root.join("link.org");
    std::os::unix::fs::symlink(&outside_file, &link).unwrap();

    let report = fixture.ops.register_all(&[root.clone()]).unwrap();
    assert_eq!(report.registered.len(), 2, "只登记可见普通文件");
    assert!(report.failed.is_empty());
    assert!(report
        .registered
        .iter()
        .any(|doc| doc.path == first.to_string_lossy()));
    assert!(report
        .registered
        .iter()
        .any(|doc| doc.path == second.to_string_lossy()));
    assert!(
        !report
            .registered
            .iter()
            .any(|doc| doc.path == hidden.to_string_lossy()),
        "隐藏项不登记"
    );
    assert!(
        !report
            .registered
            .iter()
            .any(|doc| doc.path == link.to_string_lossy()),
        "符号链接不登记"
    );
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn register_all_skips_empty_directories() {
    let fixture = Fixture::new("register-all-empty");
    let empty = fixture.path("notes/empty");
    std::fs::create_dir_all(&empty).unwrap();

    let report = fixture.ops.register_all(&[empty]).unwrap();
    assert!(report.registered.is_empty(), "空目录不产生成功项");
    assert!(report.failed.is_empty(), "空目录不算失败");
}

#[test]
fn register_all_keeps_going_after_a_failure() {
    let fixture = Fixture::new("register-all-fail");
    let good = fixture.path("notes/good.org");
    write_file(&good, "#+TITLE: good\n\ngoodword");
    let missing = fixture.path("notes/missing.org");
    let outside = outside_dir("register-all-fail");
    let outside_file = outside.join("outside.org");
    write_file(&outside_file, "#+TITLE: outside\n\noutsideword");

    let report = fixture
        .ops
        .register_all(&[good.clone(), missing.clone(), outside_file.clone()])
        .unwrap();
    assert_eq!(report.registered.len(), 1, "成功项照常登记");
    assert_eq!(report.registered[0].path, good.to_string_lossy());
    assert_eq!(report.failed.len(), 2, "失败项逐项记录");
    assert_eq!(report.failed[0].path, missing);
    assert!(report.failed[0].error.contains("existing regular file"));
    assert_eq!(report.failed[1].path, outside_file);
    assert!(report.failed[1]
        .error
        .contains("outside the data directory"));
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn register_all_is_idempotent_for_repeated_paths() {
    let fixture = Fixture::new("register-all-repeat");
    let path = fixture.path("notes/repeat.org");
    write_file(&path, "#+TITLE: repeat\n\nrepeatword");

    let report = fixture
        .ops
        .register_all(&[path.clone(), path.clone()])
        .unwrap();
    assert_eq!(report.registered.len(), 2);
    assert!(report.failed.is_empty());
    assert_eq!(
        report.registered[0].id, report.registered[1].id,
        "重复登记保留同一条记录"
    );
}
