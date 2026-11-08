// SPDX-License-Identifier: MIT

//! 事件层到知识库记录层的后台同步：只转发本地原生路径，所有 core 调用都在
//! 独立线程中完成，失败仅记录日志，不阻塞 GTK 主线程。

use std::path::PathBuf;

use crate::model::Location;

/// 复制完成后登记目标路径；目录会被 core 递归扫描。
pub(super) fn copy(locations: &[Location]) {
    let targets = native_paths(locations);
    if targets.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let Some(knowledge) = crate::knowledge::init() else {
            return;
        };
        let inside: Vec<PathBuf> = targets
            .into_iter()
            .filter(|path| knowledge.contains(path))
            .collect();
        if inside.is_empty() {
            return;
        }
        if let Err(error) = knowledge.record_copy(&inside) {
            tracing::warn!(%error, "record copied knowledge files failed");
        }
    });
}

/// 新建条目后按落点登记；目录返回 `None`，不产生记录。
pub(super) fn create(location: &Location) {
    let Some(path) = location.native_path().map(PathBuf::from) else {
        return;
    };
    std::thread::spawn(move || {
        let Some(knowledge) = crate::knowledge::init() else {
            return;
        };
        if !knowledge.contains(&path) {
            return;
        }
        if let Err(error) = knowledge.record_create(&path) {
            tracing::warn!(%error, "record created knowledge file failed");
        }
    });
}

/// 条目移动/改名后同步记录与索引；只要求目标在知识库内。
pub(super) fn move_path(from: &Location, to: &Location) {
    let (Some(from), Some(to)) = (
        from.native_path().map(PathBuf::from),
        to.native_path().map(PathBuf::from),
    ) else {
        return;
    };
    std::thread::spawn(move || {
        let Some(knowledge) = crate::knowledge::init() else {
            return;
        };
        if !knowledge.contains(&to) {
            return;
        }
        if let Err(error) = knowledge.relocate_path(&from, &to) {
            tracing::warn!(%error, "relocate knowledge document failed");
        }
    });
}

/// 永久删除完成后移除记录并移出索引；移入回收站的删除不经过这里。
pub(super) fn remove(locations: &[Location]) {
    let paths = native_paths(locations);
    if paths.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let Some(knowledge) = crate::knowledge::init() else {
            return;
        };
        for path in paths {
            if !knowledge.contains(&path) {
                continue;
            }
            if let Err(error) = knowledge.record_remove(&path) {
                tracing::warn!(%error, "remove knowledge document failed");
            }
        }
    });
}

fn native_paths(locations: &[Location]) -> Vec<PathBuf> {
    locations
        .iter()
        .filter_map(Location::native_path)
        .map(PathBuf::from)
        .collect()
}
