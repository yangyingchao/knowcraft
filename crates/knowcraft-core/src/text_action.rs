// SPDX-License-Identifier: MIT

//! 选中文字的目标识别：判定选区是 URL、邮箱还是本地绝对路径。

use std::path::{Path, PathBuf};

/// 选中文字的识别结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextTarget {
    Url(String),
    Email(String),
    Path(PathBuf),
    None,
}

/// 尾部标点集合：ASCII 的 `.,;:!?)]}'"` 与中文的 `。，、；：！？）】》""`。
const TRAILING_PUNCTUATION: &[char] = &[
    '.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '"', '。', '，', '、', '；', '：', '！', '？',
    '）', '】', '》', '“', '”',
];

/// 识别一段选中文字；没有可识别的目标时返回 [`TextTarget::None`]。
pub fn classify(text: &str) -> TextTarget {
    let text = text.trim();
    if text.is_empty() {
        return TextTarget::None;
    }
    if let Some(url) = sole_url(text) {
        return TextTarget::Url(url);
    }
    if text.chars().any(char::is_whitespace) {
        return TextTarget::None;
    }
    if let Some(email) = email_of(text) {
        return TextTarget::Email(email);
    }
    if text.starts_with('/') {
        return TextTarget::Path(PathBuf::from(text));
    }
    TextTarget::None
}

/// `path` 是否落在 `data_dir` 内。
///
/// 只做前缀比较，不解析 `..` 也不展开符号链接；这个判断只决定动作是否显示，不承担安全职责。
pub fn is_inside(data_dir: &Path, path: &Path) -> bool {
    path.starts_with(data_dir)
}

/// 整串里恰好出现的那个 http/https URL；出现零个或多个时返回 `None`。
fn sole_url(text: &str) -> Option<String> {
    let mut found: Option<String> = None;
    let mut index = 0;
    while index < text.len() {
        if let Some(candidate) = url_starting_at(text, index) {
            if found.is_some() {
                return None;
            }
            found = Some(candidate);
        }
        index += text[index..].chars().next().map_or(1, char::len_utf8);
    }
    found
}

/// `text[start..]` 以一个 http/https URL 开头时返回剥离尾部标点后的 URL。
fn url_starting_at(text: &str, start: usize) -> Option<String> {
    let rest = &text[start..];
    if !rest.starts_with("http://") && !rest.starts_with("https://") {
        return None;
    }
    let mut end = start;
    for (offset, ch) in rest.char_indices() {
        if !is_url_char(ch) {
            break;
        }
        end = start + offset + ch.len_utf8();
    }
    let candidate = trim_trailing_punctuation(&text[start..end]);
    let parsed = url::Url::parse(candidate).ok()?;
    if !parsed.has_host() || !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    Some(candidate.to_owned())
}

/// URL 字符集：RFC 3986 的 unreserved 与 reserved，再加 `%`。
fn is_url_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(
            ch,
            '-' | '.'
                | '_'
                | '~'
                | ':'
                | '/'
                | '?'
                | '#'
                | '['
                | ']'
                | '@'
                | '!'
                | '$'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | '+'
                | ','
                | ';'
                | '='
                | '%'
        )
}

fn trim_trailing_punctuation(text: &str) -> &str {
    text.trim_end_matches(|ch| TRAILING_PUNCTUATION.contains(&ch))
}

/// 形如 `local@domain.tld` 的邮箱；只校验单个 `@` 与域名里的点。
fn email_of(text: &str) -> Option<String> {
    let text = trim_trailing_punctuation(text.strip_prefix("mailto:").unwrap_or(text));
    let (local, domain) = text.split_once('@')?;
    if local.is_empty() || domain.contains('@') || !domain.contains('.') {
        return None;
    }
    if local.contains('/') || domain.contains('/') {
        return None;
    }
    Some(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> TextTarget {
        TextTarget::Url(text.to_owned())
    }

    fn email(text: &str) -> TextTarget {
        TextTarget::Email(text.to_owned())
    }

    #[test]
    fn classify_reads_each_target_kind() {
        assert_eq!(
            classify("https://example.com"),
            TextTarget::Url("https://example.com".into())
        );
        assert_eq!(classify("a@b.com"), TextTarget::Email("a@b.com".into()));
        assert_eq!(
            classify("/tmp/a.pdf"),
            TextTarget::Path(PathBuf::from("/tmp/a.pdf"))
        );
    }

    #[test]
    fn classify_strips_ascii_trailing_punctuation() {
        assert_eq!(classify("https://example.com,"), url("https://example.com"));
        assert_eq!(
            classify("https://example.com/a?b=1)."),
            url("https://example.com/a?b=1")
        );
        assert_eq!(classify("a@b.com)"), email("a@b.com"));
    }

    #[test]
    fn classify_reads_a_url_before_chinese_punctuation() {
        assert_eq!(
            classify("见https://example.com。我们"),
            url("https://example.com")
        );
        assert_eq!(
            classify("见 https://example.com 我们"),
            url("https://example.com")
        );
    }

    #[test]
    fn classify_reads_mailto_with_a_bare_address() {
        assert_eq!(classify("mailto:a@b.com"), email("a@b.com"));
    }

    #[test]
    fn classify_rejects_relative_paths_and_plain_text() {
        assert_eq!(classify("~/x"), TextTarget::None);
        assert_eq!(classify("api/v1/users"), TextTarget::None);
        assert_eq!(classify("hello world"), TextTarget::None);
        assert_eq!(classify("/tmp/a@b.pdf"), TextTarget::Path(PathBuf::from("/tmp/a@b.pdf")));
    }

    #[test]
    fn classify_requires_exactly_one_url() {
        assert_eq!(classify("http://a.com http://b.com"), TextTarget::None);
        assert_eq!(classify("http://a.com,http://b.com"), TextTarget::None);
        assert_eq!(classify("http://"), TextTarget::None);
    }

    #[test]
    fn is_inside_matches_the_directory_prefix() {
        assert!(is_inside(Path::new("/lib"), Path::new("/lib/a/x")));
        assert!(is_inside(Path::new("/lib"), Path::new("/lib")));
        assert!(!is_inside(Path::new("/lib"), Path::new("/other/x")));
        assert!(!is_inside(Path::new("/lib"), Path::new("/libx/x")));
    }
}
