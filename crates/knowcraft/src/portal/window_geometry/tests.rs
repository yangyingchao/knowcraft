// SPDX-License-Identifier: MIT

mod centering;

use super::*;
use std::os::unix::net::UnixListener;

#[test]
fn socket_paths_stay_inside_the_runtime_instance() {
    assert_eq!(
        socket_path(Path::new("/run/user/1000"), "abc_123-def"),
        Some(PathBuf::from(
            "/run/user/1000/hypr/abc_123-def/.socket.sock"
        ))
    );
    for signature in ["", "..", "../other", "/tmp/socket", "a/b", "a\0b"] {
        assert_eq!(socket_path(Path::new("/run/user/1000"), signature), None);
    }
    assert_eq!(socket_path(Path::new("relative"), "abc"), None);
}
