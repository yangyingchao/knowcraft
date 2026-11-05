use std::net::IpAddr;
use std::path::PathBuf;

use scraper::{ElementRef, Html, Selector};
use url::{Host, Url};

use crate::error::{Error, Result};
use crate::model::Document;
use crate::store::{now, Store};

const REQUEST_TIMEOUT_SECS: u64 = 30;

pub struct Capture {
    store: Store,
    data_dir: PathBuf,
    client: reqwest::blocking::Client,
}

impl Capture {
    pub fn new(store: Store, data_dir: PathBuf) -> Result<Capture> {
        // 关闭自动重定向：防止 302 跳转到内网地址绕过初始 URL 的 SSRF 检查。
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .map_err(|e| Error::Capture(e.to_string()))?;
        Ok(Capture {
            store,
            data_dir,
            client,
        })
    }

    pub fn fetch(&self, url: &str) -> Result<Document> {
        let parsed = Url::parse(url).map_err(|e| Error::Capture(e.to_string()))?;
        if is_blocked(&parsed) {
            return Err(Error::Capture("blocked address".into()));
        }

        let resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| Error::Network(e.to_string()))?;
        // Policy::none() 下 3xx 作为普通响应返回：显式拒绝，不访问 Location。
        if resp.status().is_redirection() {
            return Err(Error::Capture(format!(
                "redirect blocked: {}",
                resp.status()
            )));
        }
        let html = resp.text().map_err(|e| Error::Network(e.to_string()))?;
        let (title, markdown) = extract_article(&html, &parsed)?;

        let cap_dir = self.data_dir.join("captures");
        std::fs::create_dir_all(&cap_dir)?;
        let md_path = cap_dir.join(format!("{}.md", sanitize(&title)));
        let tmp_path = cap_dir.join(format!(".{}.{}.tmp", sanitize(&title), std::process::id()));
        std::fs::write(&tmp_path, markdown.as_bytes())?;
        std::fs::rename(&tmp_path, &md_path)?;

        let ts = now();
        let doc = Document {
            id: 0,
            path: md_path.to_string_lossy().into_owned(),
            doc_type: "md".into(),
            title,
            source_url: Some(url.to_string()),
            size: std::fs::metadata(&md_path)?.len() as i64,
            created_at: ts.clone(),
            updated_at: ts,
            checksum: String::new(),
            ai_summary: None,
            lifecycle: "pending".into(),
            index_status: "pending".into(),
        };
        let id = self.store.insert_document(&doc)?;
        self.store
            .get_document(id)?
            .ok_or_else(|| Error::Store("missing document after insert".into()))
    }
}

/// 按 URL 字面 host 判断是否为禁止抓取地址。
/// 不做 DNS 解析：域名解析到内网的 DNS rebinding 不在本任务防护范围内。
fn is_blocked(u: &Url) -> bool {
    match u.host() {
        None => true,
        Some(Host::Domain(h)) => {
            // 去尾点并小写：localhost. 与 LOCALHOST 是同一主机。
            let h = h.trim_end_matches('.').to_ascii_lowercase();
            if h == "localhost" {
                return true;
            }
            // 非标准数字编码标准库 IpAddr 拒绝但系统解析器接受，一律拦截。
            if is_ambiguous_numeric_host(&h) {
                return true;
            }
            match h.parse::<IpAddr>() {
                Ok(ip) => is_blocked_ip(ip),
                Err(_) => false,
            }
        }
        Some(Host::Ipv4(v4)) => is_blocked_ip(IpAddr::V4(v4)),
        Some(Host::Ipv6(v6)) => is_blocked_ip(IpAddr::V6(v6)),
    }
}

/// 检测系统解析器接受但标准库 IpAddr 拒绝的数字主机写法。
/// 保守策略：疑似数字 IP 但无法按标准形式解析的主机一律拦截。
fn is_ambiguous_numeric_host(h: &str) -> bool {
    let labels: Vec<&str> = h.split('.').filter(|l| !l.is_empty()).collect();
    if labels.is_empty() {
        return true;
    }
    // 仅当某个标签整体为十六进制整数时拦截（0x7f、0x7f000001）。
    // 不做子串匹配：10xgenomics.com 等含 0x 的普通域名必须放行。
    // h 已在调用方小写化，只需匹配 0x 前缀。
    if labels.iter().any(|l| {
        l.strip_prefix("0x")
            .map(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit()))
            .unwrap_or(false)
    }) {
        return true;
    }
    if !labels.iter().all(|l| l.chars().all(|c| c.is_ascii_digit())) {
        return false;
    }
    // 单标签整数形式：2130706433。
    if labels.len() == 1 {
        return true;
    }
    // 前导零八进制歧义：0177.0.0.1。
    if labels.iter().any(|l| l.len() > 1 && l.starts_with('0')) {
        return true;
    }
    // 非四段点分形式（如 1.2.3，系统按 1.2.0.3 解析）或标准解析失败，拦截。
    if labels.len() != 4 {
        return true;
    }
    h.parse::<std::net::Ipv4Addr>().is_err()
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    // IPv4-mapped IPv6（如 ::ffff:127.0.0.1）按映射后的 IPv4 判定。
    let ip = match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        other => other,
    };
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_unspecified()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                // 基准测试网段 198.18.0.0/15（is_benchmarking 在本工具链尚未稳定）
                || (v4.octets()[0] == 198
                    && (v4.octets()[1] == 18 || v4.octets()[1] == 19))
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
        }
    }
}

/// 从 HTML 提取正文，返回 (标题, markdown)。优先选取 article/main，回退 body。
pub fn extract_article(html: &str, _base: &Url) -> Result<(String, String)> {
    let doc = Html::parse_document(html);

    let container_sel =
        Selector::parse("article, main, body").map_err(|e| Error::Capture(e.to_string()))?;
    let heading_sel =
        Selector::parse("h1, h2, h3, h4").map_err(|e| Error::Capture(e.to_string()))?;
    let body_sel = Selector::parse("h1, h2, h3, h4, p, blockquote, pre, li")
        .map_err(|e| Error::Capture(e.to_string()))?;
    let title_sel = Selector::parse("title").map_err(|e| Error::Capture(e.to_string()))?;

    let root = doc
        .select(&container_sel)
        .next()
        .ok_or_else(|| Error::Capture("no document body".into()))?;

    let title_el = root.select(&heading_sel).next();
    let title = match title_el {
        Some(el) => node_text(el),
        None => match doc.select(&title_sel).next() {
            Some(el) => node_text(el),
            None => "Untitled".to_string(),
        },
    };
    let title = collapse(&title);
    let title = if title.is_empty() {
        "Untitled".to_string()
    } else {
        title
    };

    let title_id = title_el.map(|el| el.id());
    let mut md = format!("# {title}\n");
    for el in root.select(&body_sel) {
        if Some(el.id()) == title_id {
            continue;
        }
        let text = collapse(&node_text(el));
        if text.is_empty() {
            continue;
        }
        let line = match el.value().name() {
            "h1" => format!("\n# {text}\n"),
            "h2" => format!("\n## {text}\n"),
            "h3" => format!("\n### {text}\n"),
            "h4" => format!("\n#### {text}\n"),
            "blockquote" => format!("\n> {text}\n"),
            "li" => format!("- {text}"),
            _ => format!("\n{text}\n"),
        };
        md.push_str(&line);
    }

    Ok((title, md))
}

fn node_text(el: ElementRef<'_>) -> String {
    el.text().collect::<Vec<_>>().join(" ")
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 转换为合法文件 stem：非法字符替换为下划线，处理空标题与保留名。
fn sanitize(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name: String = collapse(&cleaned)
        .trim_matches(['.', ' '])
        .chars()
        .take(120)
        .collect();
    if name.is_empty() {
        "untitled".to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked(url: &str) -> bool {
        is_blocked(&Url::parse(url).expect("test url must parse"))
    }

    #[test]
    fn ssrf_table_driven() {
        let blocked_cases: &[&str] = &[
            // 无 host
            "file:///etc/passwd",
            "ftp://127.0.0.1/path",
            // IPv4 环回及各类保留/私有地址
            "http://127.0.0.1/",
            "http://127.1.2.3/",
            "http://10.0.0.1/",
            "http://172.16.0.1/",
            "http://172.31.255.255/",
            "http://192.168.1.1/",
            "http://0.0.0.0/",
            "http://169.254.1.1/",
            "http://255.255.255.255/",
            "http://192.0.2.9/",
            "http://198.51.100.9/",
            "http://203.0.113.9/",
            "http://198.18.0.1/",
            "http://198.19.255.255/",
            // IPv6 环回/未指定/ULA/链路本地
            "http://[::1]/",
            "http://[::]/",
            "http://[fc00::1]/",
            "http://[fdff::1]/",
            "http://[fe80::1]/",
            // IPv4-mapped IPv6
            "http://[::ffff:127.0.0.1]/",
            "http://[::ffff:10.0.0.1]/",
            // localhost 变体
            "http://localhost/",
            "http://localhost.:8080/",
            "http://LOCALHOST./",
            // 非标准数字编码
            "http://0x7f000001/",
            "http://0x7f.0.0.1/",
            "http://0177.0.0.1/",
            "http://2130706433/",
        ];
        for c in blocked_cases {
            assert!(blocked(c), "expected blocked: {c}");
        }

        let allowed_cases: &[&str] = &[
            "http://example.com/",
            "https://blog.example.org/post",
            "http://sub.domain.example.net:8080/x",
            // 含 0x 子串但非十六进制整数标签的合法域名
            "http://10xgenomics.com/",
            "http://www.10xgenomics.com/",
            "http://0x-com.org/",
            "http://a0x.example.com/",
            "http://93.184.216.34/",
            "http://8.8.8.8/",
            "http://172.32.0.1/",
            "http://172.15.0.1/",
            "http://11.0.0.1/",
            "http://192.169.0.1/",
            "http://198.17.0.1/",
            "http://198.20.0.1/",
            "http://[2606:4700:4700::1111]/",
        ];
        for c in allowed_cases {
            assert!(!blocked(c), "expected allowed: {c}");
        }
    }
}
