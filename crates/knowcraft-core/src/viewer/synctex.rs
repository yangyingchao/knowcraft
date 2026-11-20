//! Minimal FFI binding for the bundled SyncTeX 1.21 parser.

use std::ffi::{c_char, c_float, c_int, CStr, CString};

#[repr(C)]
struct SynctexScannerOpaque {
    _private: [u8; 0],
}

#[repr(C)]
struct SynctexNodeOpaque {
    _private: [u8; 0],
}

type Scanner = SynctexScannerOpaque;
type Node = SynctexNodeOpaque;

extern "C" {
    fn synctex_scanner_new_with_output_file(
        output: *const c_char,
        build_directory: *const c_char,
        parse: c_int,
    ) -> *mut Scanner;
    fn synctex_scanner_free(scanner: *mut Scanner) -> c_int;
    fn synctex_display_query(
        scanner: *mut Scanner,
        name: *const c_char,
        line: c_int,
        column: c_int,
        page_hint: c_int,
    ) -> i64;
    fn synctex_edit_query(scanner: *mut Scanner, page: c_int, h: c_float, v: c_float) -> i64;
    fn synctex_scanner_next_result(scanner: *mut Scanner) -> *mut Node;
    fn synctex_node_page(node: *mut Node) -> c_int;
    fn synctex_node_box_visible_h(node: *mut Node) -> c_float;
    fn synctex_node_box_visible_v(node: *mut Node) -> c_float;
    fn synctex_node_box_visible_width(node: *mut Node) -> c_float;
    fn synctex_node_box_visible_height(node: *mut Node) -> c_float;
    fn synctex_node_box_visible_depth(node: *mut Node) -> c_float;
    fn synctex_node_tag(node: *mut Node) -> c_int;
    fn synctex_node_line(node: *mut Node) -> c_int;
    fn synctex_node_column(node: *mut Node) -> c_int;
    fn synctex_scanner_get_name(scanner: *mut Scanner, tag: c_int) -> *const c_char;
}

/// A scanner that frees itself. Nodes borrow from it and must not outlive it.
pub struct ScannerHandle {
    raw: *mut Scanner,
}

impl ScannerHandle {
    pub fn new(output: &str) -> Option<Self> {
        let c = CString::new(output).ok()?;
        // SAFETY: `c` is a valid NUL terminated path; a null build directory means "none".
        let raw = unsafe { synctex_scanner_new_with_output_file(c.as_ptr(), std::ptr::null(), 1) };
        if raw.is_null() {
            None
        } else {
            Some(Self { raw })
        }
    }

    pub fn display_query(&mut self, name: &str, line: i32, column: i32) -> bool {
        let Ok(c) = CString::new(name) else {
            return false;
        };
        // SAFETY: the scanner is live and `c` is a valid NUL terminated string.
        let status = unsafe { synctex_display_query(self.raw, c.as_ptr(), line, column, 0) };
        status != 0
    }

    pub fn edit_query(&mut self, page: i32, h: f32, v: f32) -> bool {
        // SAFETY: the scanner is live.
        unsafe { synctex_edit_query(self.raw, page, h, v) != 0 }
    }

    pub fn next_result(&self) -> Option<NodeHandle<'_>> {
        // SAFETY: the scanner is live.
        let node = unsafe { synctex_scanner_next_result(self.raw) };
        if node.is_null() {
            None
        } else {
            Some(NodeHandle {
                raw: node,
                _scanner: std::marker::PhantomData,
            })
        }
    }

    pub fn name_for_tag(&self, tag: i32) -> Option<String> {
        // SAFETY: the scanner is live; the returned pointer belongs to it.
        let p = unsafe { synctex_scanner_get_name(self.raw, tag) };
        if p.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        }
    }
}

impl Drop for ScannerHandle {
    fn drop(&mut self) {
        // SAFETY: the scanner was created by the library and is freed exactly once.
        unsafe {
            synctex_scanner_free(self.raw);
        }
    }
}

/// A result node, valid while its scanner is alive.
pub struct NodeHandle<'a> {
    raw: *mut Node,
    _scanner: std::marker::PhantomData<&'a ScannerHandle>,
}

impl NodeHandle<'_> {
    pub fn page(&self) -> i32 {
        // SAFETY: the node points into a live scanner.
        unsafe { synctex_node_page(self.raw) }
    }

    pub fn visible_h(&self) -> f32 {
        unsafe { synctex_node_box_visible_h(self.raw) }
    }

    pub fn visible_v(&self) -> f32 {
        unsafe { synctex_node_box_visible_v(self.raw) }
    }

    pub fn visible_width(&self) -> f32 {
        unsafe { synctex_node_box_visible_width(self.raw) }
    }

    pub fn visible_height(&self) -> f32 {
        unsafe { synctex_node_box_visible_height(self.raw) }
    }

    pub fn visible_depth(&self) -> f32 {
        unsafe { synctex_node_box_visible_depth(self.raw) }
    }

    pub fn tag(&self) -> i32 {
        unsafe { synctex_node_tag(self.raw) }
    }

    pub fn line(&self) -> i32 {
        unsafe { synctex_node_line(self.raw) }
    }

    pub fn column(&self) -> i32 {
        unsafe { synctex_node_column(self.raw) }
    }
}
