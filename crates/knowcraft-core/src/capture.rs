use std::net::IpAddr;
use std::path::PathBuf;

use scraper::{ElementRef, Html, Selector};
use url::{Host, Url};

use crate::error::{Error, Result};
use crate::model::Document;
use crate::store::{now, Store};

pub struct Capture {
    store: Store,
    data_dir: PathBuf,
}

impl Capture {
    pub fn new(store: Store, data_dir: PathBuf) -> Result<Capture> {
        Ok(Capture { store, data_dir })
    }

    pub fn fetch(&self, url: &str) -> Result<Document> {
        let parsed = Url::parse(url).map_err(|e| Error::Capture(e.to_string()))?;
        if is_blocked(&parsed) {
            return Err(Error::Capture("blocked address".into()));
        }

        let resp = reqwest::blocking::get(url).map_err(|e| Error::Network(e.to_string()))?;
        let html = resp.text().map_err(|e| Error::Network(e.to_string()))?;
        let (title, markdown) = extract_article(&html, &parsed)?;

        let cap_dir = self.data_dir.join("captures");
        std::fs::create_dir_all(&cap_dir)?;
        let md_path = cap_dir.join(format!("{}.md", sanitize(&title)));
        let tmp_path = cap_dir.join(format!(
            ".{}.{}.tmp",
            sanitize(&title),
            std::process::id()
        ));
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
        Some(Host::Domain("localhost")) => true,
        Some(Host::Domain(h)) => match h.parse::<IpAddr>() {
            Ok(ip) => is_blocked_ip(ip),
            Err(_) => false,
        },
        Some(Host::Ipv4(v4)) => is_blocked_ip(IpAddr::V4(v4)),
        Some(Host::Ipv6(v6)) => is_blocked_ip(IpAddr::V6(v6)),
    }
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

    let container_sel = Selector::parse("article, main, body")
        .map_err(|e| Error::Capture(e.to_string()))?;
    let heading_sel = Selector::parse("h1, h2, h3, h4")
        .map_err(|e| Error::Capture(e.to_string()))?;
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
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                || c.is_control()
            {
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
