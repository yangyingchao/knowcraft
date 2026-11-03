use knowcraft_core::capture::{extract_article, Capture};
use knowcraft_core::store::Store;
use url::Url;

#[test]
fn rejects_loopback_url() {
    let dir = std::env::temp_dir().join(format!("kc-cap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let cap = Capture::new(store, dir.clone()).unwrap();
    let err = cap.fetch("http://127.0.0.1:8080/secret").unwrap_err();
    assert!(err.to_string().contains("blocked"));
}

#[test]
fn rejects_encoded_loopback_hosts() {
    let dir = std::env::temp_dir().join(format!("kc-cap-hex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let cap = Capture::new(store, dir).unwrap();
    // 十六进制与八进制数字编码在发起网络前拦截，不触网。
    let err = cap.fetch("http://0x7f000001/").unwrap_err();
    assert!(err.to_string().contains("blocked"));
    let err = cap.fetch("http://0177.0.0.1/").unwrap_err();
    assert!(err.to_string().contains("blocked"));
}

#[test]
fn rejects_trailing_dot_localhost() {
    let dir = std::env::temp_dir().join(format!("kc-cap-ld-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let cap = Capture::new(store, dir).unwrap();
    let err = cap.fetch("http://localhost./").unwrap_err();
    assert!(err.to_string().contains("blocked"));
}

#[test]
fn parses_local_html_to_markdown() {
    // 离线断言：直接调用纯函数 extract_article，不经过网络。
    // 本地 HTTP server 只能绑定 127.0.0.1，与 SSRF 防护冲突，故不采用。
    let html = include_str!("fixtures/page.html");
    let base = Url::parse("https://example.com/post").unwrap();
    let (title, markdown) = extract_article(html, &base).unwrap();
    assert_eq!(title, "Local Fixture Article");
    assert!(markdown.contains("# Local Fixture Article"));
    assert!(markdown.contains("First body paragraph."));
    assert!(markdown.contains("Second body paragraph."));
    // 正文之外的噪声不进入提取结果。
    assert!(!markdown.contains("footer noise"));
}
