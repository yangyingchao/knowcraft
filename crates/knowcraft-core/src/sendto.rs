// SPDX-License-Identifier: MIT
//! 外部 provider 的同步发送门面。
//!
//! GUI 的 Send to 菜单与通知保留在 GUI crate；本模块只负责协议交互，供 CLI 与
//! 后续 HTTP/MCP 复用。每次发送都新建 provider 连接，返回前关闭，避免遗留子进程。

use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

use crate::{
    error::{Error, Result},
    file_providers::{
        config_root, discover,
        protocol::{query_batch, MenuAction, OutcomeStatus, Reply, Request},
        start, Client, Registration, Update,
    },
};

/// 单个 provider 回复的等待上限，与 transport 的 deadline 一致。
const REPLY_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentAction {
    pub id: String,
    pub label: String,
    pub context: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendOutcome {
    pub provider: String,
    pub action: String,
    pub accepted: usize,
    pub total: usize,
    pub message: String,
}

pub struct SendTo {
    registrations: Vec<Registration>,
}

impl SendTo {
    /// 读取默认 provider 配置目录。
    pub fn discover() -> Self {
        let registrations = config_root().map_or_else(Vec::new, |root| discover(&root));
        Self { registrations }
    }

    /// 指定配置目录，供测试与自定义部署使用。
    pub fn with_root(root: PathBuf) -> Self {
        Self {
            registrations: discover(&root),
        }
    }

    /// 已注册 provider，按 id 排序；name 缺失时回退为 id。
    pub fn providers(&self) -> Vec<ProviderSummary> {
        let mut providers: Vec<_> = self
            .registrations
            .iter()
            .map(|registration| ProviderSummary {
                id: registration.manifest.id.clone(),
                name: registration
                    .manifest
                    .name
                    .clone()
                    .unwrap_or_else(|| registration.manifest.id.clone()),
            })
            .collect();
        providers.sort_by(|a, b| a.id.cmp(&b.id));
        providers
    }

    /// 指定路径下 provider 暴露的全部发送动作。
    pub fn actions_for(&self, provider_id: &str, paths: &[PathBuf]) -> Result<Vec<SentAction>> {
        let registration = self.registration(provider_id)?;
        let paths = native_paths(paths)?;
        let client = start(registration);
        let mut serial = 0;
        query_actions(&client, provider_id, &paths, &mut serial)
    }

    /// 自动选择唯一的发送动作并把路径交给 provider。
    pub fn send(&self, provider_id: &str, paths: &[PathBuf]) -> Result<SendOutcome> {
        let registration = self.registration(provider_id)?;
        let paths = native_paths(paths)?;
        let client = start(registration);
        let mut serial = 0;
        let actions = query_actions(&client, provider_id, &paths, &mut serial)?;
        let action = match actions.as_slice() {
            [] => {
                return Err(Error::SendTo(format!(
                    "provider {provider_id} offers no send-to action"
                )));
            }
            [action] => action.clone(),
            [_, ..] => {
                let listing: Vec<_> = actions
                    .iter()
                    .map(|action| format!("{}={}", action.id, action.label))
                    .collect();
                return Err(Error::SendTo(format!(
                    "provider {provider_id} offers multiple send-to actions: {}",
                    listing.join(", ")
                )));
            }
        };

        let mut accepted = 0;
        let mut total = 0;
        let mut message = String::new();
        for batch in batches(&paths)? {
            serial += 1;
            let mut request = request("activate", batch.clone(), serial);
            request.action = Some(action.id.clone());
            request.context = action.context.clone();
            client.requests.try_send(request).map_err(|_| {
                Error::SendTo(format!("provider {provider_id} is busy or unavailable"))
            })?;
            let reply = wait_reply(&client, provider_id)?;
            let outcome = reply.outcome.ok_or_else(|| {
                Error::SendTo(format!("provider {provider_id} returned no outcome"))
            })?;
            let counts = match outcome.accepted.zip(outcome.total) {
                Some((batch_accepted, batch_total)) => {
                    if outcome.status == OutcomeStatus::Accepted
                        && batch_accepted == batch_total
                        && batch_total == batch.len()
                    {
                        accepted += batch_accepted;
                        total += batch_total;
                        message = reply.message.clone();
                        continue;
                    }
                    format!("{batch_accepted}/{batch_total}")
                }
                None => "-".to_owned(),
            };
            return Err(Error::SendTo(format!(
                "provider {provider_id} action {} failed: {} accepted {counts} of {}",
                action.id,
                status_name(&outcome.status),
                batch.len()
            )));
        }
        Ok(SendOutcome {
            provider: provider_id.to_owned(),
            action: action.id,
            accepted,
            total,
            message,
        })
    }

    fn registration(&self, provider_id: &str) -> Result<Registration> {
        self.registrations
            .iter()
            .find(|registration| registration.manifest.id == provider_id)
            .cloned()
            .ok_or_else(|| Error::SendTo(format!("provider {provider_id} is not registered")))
    }
}

fn request(method: &str, paths: Vec<String>, id: u64) -> Request {
    Request {
        version: 1,
        id,
        method: method.into(),
        paths,
        background: false,
        action: None,
        context: None,
    }
}

/// 询问 provider 在给定路径下暴露的发送动作；多批结果按 id 去重。
fn query_actions(
    client: &Client,
    provider_id: &str,
    paths: &[String],
    serial: &mut u64,
) -> Result<Vec<SentAction>> {
    let mut actions = Vec::new();
    for batch in batches(paths)? {
        *serial += 1;
        client
            .requests
            .try_send(request("menu", batch, *serial))
            .map_err(|_| Error::SendTo(format!("provider {provider_id} is busy or unavailable")))?;
        let reply = wait_reply(client, provider_id)?;
        collect_send_actions(&reply.actions.unwrap_or_default(), &mut actions);
    }
    actions.sort_by(|a, b| a.id.cmp(&b.id));
    actions.dedup_by(|a, b| a.id == b.id);
    Ok(actions)
}

fn collect_send_actions(actions: &[MenuAction], out: &mut Vec<SentAction>) {
    for action in actions {
        if let Some(children) = &action.children {
            collect_send_actions(children, out);
        } else if action.sends_to() {
            out.push(SentAction {
                id: action.id.clone(),
                label: action.label.clone(),
                context: action.context.clone(),
            });
        }
    }
}

/// 按协议帧上限把路径切成批次。
fn batches(paths: &[String]) -> Result<Vec<Vec<String>>> {
    let mut out = Vec::new();
    let mut rest = paths.to_vec();
    while !rest.is_empty() {
        let batch = query_batch(rest.clone());
        if batch.is_empty() {
            return Err(Error::SendTo(
                "path selection exceeds the provider frame limit".into(),
            ));
        }
        rest.drain(..batch.len());
        out.push(batch);
    }
    Ok(out)
}

fn wait_reply(client: &Client, provider_id: &str) -> Result<Reply> {
    let deadline = Instant::now() + REPLY_TIMEOUT;
    loop {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return Err(Error::SendTo(format!(
                "provider {provider_id} did not reply in time"
            )));
        };
        match client.updates.recv_timeout(left) {
            Ok(Update::Reply(reply)) => {
                if reply.event.is_some() {
                    continue;
                }
                if let Some(error) = reply.error.clone() {
                    return Err(Error::SendTo(format!(
                        "provider {provider_id} rejected the request: {error}"
                    )));
                }
                return Ok(*reply);
            }
            Ok(Update::Offline { .. }) => {
                return Err(Error::SendTo(format!("provider {provider_id} is offline")));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(Error::SendTo(format!(
                    "provider {provider_id} did not reply in time"
                )));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Error::SendTo(format!(
                    "provider {provider_id} disconnected"
                )));
            }
        }
    }
}

fn native_paths(paths: &[PathBuf]) -> Result<Vec<String>> {
    paths
        .iter()
        .map(|path| {
            if !path.is_absolute() {
                return Err(Error::SendTo(format!(
                    "path must be absolute: {}",
                    path.display()
                )));
            }
            path.to_str().map(str::to_owned).ok_or_else(|| {
                Error::SendTo(format!("path is not valid UTF-8: {}", path.display()))
            })
        })
        .collect()
}

fn status_name(status: &OutcomeStatus) -> &'static str {
    match status {
        OutcomeStatus::Accepted => "accepted",
        OutcomeStatus::Rejected => "rejected",
        OutcomeStatus::Partial => "partial",
        OutcomeStatus::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests;
