// SPDX-License-Identifier: MIT

//! Column views and list columns: widths, order, visibility, and sorting.
//!
//! 这些值存在 `state.toml`（窗口自己产生的布局状态），此处只保留调用入口。

use super::*;

impl PreferenceManager {
    pub fn chooser_column_width(&self) -> Option<i32> {
        ui_state::chooser_column_width()
    }

    pub fn set_chooser_column_width(&self, width: Option<i32>) {
        ui_state::set_chooser_column_width(width);
    }

    pub fn browser_column_width(&self) -> Option<i32> {
        ui_state::browser_column_width()
    }

    pub fn set_browser_column_width(&self, width: Option<i32>) {
        ui_state::set_browser_column_width(width);
    }

    pub fn browser_list_columns(&self) -> Option<ListColumns> {
        ui_state::browser_list_columns()
    }

    pub fn set_browser_list_columns(&self, columns: Option<ListColumns>) {
        ui_state::set_browser_list_columns(columns.filter(ListColumns::is_valid));
    }

    pub fn chooser_list_columns(&self) -> Option<ListColumns> {
        ui_state::chooser_list_columns()
    }

    pub fn set_chooser_list_columns(&self, columns: Option<ListColumns>) {
        ui_state::set_chooser_list_columns(columns);
    }

    pub fn list_column_order(&self) -> Vec<ListColumnId> {
        ui_state::list_column_order()
    }

    pub fn set_list_column_order(&self, order: Vec<ListColumnId>) {
        ui_state::set_list_column_order(order);
    }

    pub fn list_hidden_columns(&self) -> Vec<ListColumnId> {
        ui_state::list_hidden_columns()
    }

    pub fn set_list_hidden_columns(&self, hidden: Vec<ListColumnId>) {
        ui_state::set_list_hidden_columns(hidden);
    }

    pub fn is_list_column_visible(&self, column: ListColumnId) -> bool {
        if column == ListColumnId::Name {
            return true;
        }
        !self.list_hidden_columns().contains(&column)
    }

    pub fn set_list_column_visible(&self, column: ListColumnId, visible: bool) {
        if column == ListColumnId::Name {
            return;
        }
        let mut hidden = self.list_hidden_columns();
        if visible {
            hidden.retain(|col| *col != column);
        } else if !hidden.contains(&column) {
            hidden.push(column);
        }
        self.set_list_hidden_columns(hidden);
    }

    pub fn move_list_column(&self, column: ListColumnId, target: ListColumnId, after: bool) {
        let mut order = self.list_column_order();
        let Some(from) = order.iter().position(|item| *item == column) else {
            return;
        };
        let Some(target) = order.iter().position(|item| *item == target) else {
            return;
        };
        let mut to = target + usize::from(after);
        if from < to {
            to -= 1;
        }
        if from != to {
            let column = order.remove(from);
            order.insert(to, column);
            self.set_list_column_order(order);
        }
    }

    pub fn reset_list_columns(&self) {
        ui_state::set_list_column_order(default_list_column_order());
        ui_state::set_list_hidden_columns(Vec::new());
    }

    pub fn list_visible_columns(&self) -> Vec<ListColumnId> {
        let hidden = self.list_hidden_columns();
        self.list_column_order()
            .into_iter()
            .filter(|col| *col == ListColumnId::Name || !hidden.contains(col))
            .collect()
    }

    pub fn sort_preferences(&self) -> ViewPreferences {
        sort_preferences(&self.preferences.borrow())
    }

    /// Unlike `set_default_sort`, this keeps folder sorts that equal the new
    /// default: with per-folder sorts off, they are kept but ignored.
    pub fn set_sort_preferences(&self, preferences: ViewPreferences) {
        if preferences.sort_key == SortKey::Recency {
            return;
        }
        ui_state::set_show_hidden(preferences.show_hidden);
        let mut stored = self.preferences.borrow_mut();
        stored.folders_first = preferences.folders_first;
        store_sort(&mut stored, preferences.sort());
        drop(stored);
        self.save_preferences();
    }
}
