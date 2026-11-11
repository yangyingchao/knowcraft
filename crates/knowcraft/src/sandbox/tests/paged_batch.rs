// SPDX-License-Identifier: MIT

use super::super::batch_index;

#[test]
fn batch_file_names_are_whitelisted() {
    assert_eq!(batch_index("result.png", "png"), Some(0));
    assert_eq!(batch_index("result.text", "text"), Some(0));
    assert_eq!(batch_index("result.1.png", "png"), Some(1));
    assert_eq!(batch_index("result.3.text", "text"), Some(3));
    assert_eq!(batch_index("result.0.png", "png"), None);
    assert_eq!(batch_index("result.9.png", "png"), None);
    assert_eq!(batch_index("../result.png", "png"), None);
    assert_eq!(batch_index("/etc/passwd", "png"), None);
    assert_eq!(batch_index("result/a.png", "png"), None);
    assert_eq!(batch_index("result.png.bak", "png"), None);
    assert_eq!(batch_index("result.1.png", "text"), None);
}
