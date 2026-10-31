//! Saved SSH connections and remote workspace layouts.
//!
//! Remote layouts only accept terminal and split nodes. Local layout bytes and
//! repair rules stay in `layout`.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use crate::layout::Direction;
use crate::validate_name;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr};

pub const MAX_REMOTE_LAYOUT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SshAuthType {
    Password,
    PrivateKey,
}

impl SshAuthType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::PrivateKey => "privateKey",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "password" => Ok(Self::Password),
            "privateKey" => Ok(Self::PrivateKey),
            _ => Err("认证方式只支持密码或私钥".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConnectionInput {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: SshAuthType,
    pub private_key_path: Option<String>,
}

/// Normalize a host for storage and known-host lookup.
///
/// DNS names are lowercased. IPv6 text forms, with or without brackets, collapse
/// to one compressed spelling. The configured DNS name is not replaced by a
/// resolved address.
pub fn normalize_endpoint(host: &str, port: Option<u16>) -> Result<(String, u16), String> {
    let host = host.trim();
    if host.is_empty() || host.len() > 253 || host.contains(['\0', ' ', '\t', '\n', '\r']) {
        return Err("主机名无效".into());
    }
    if host.contains('@')
        || host.contains("://")
        || host.starts_with('-')
        || host.contains(['\\', '?', '#', '/'])
    {
        return Err("主机只能是 DNS 名称或 IP 地址，不能包含用户名、URL 或选项".into());
    }
    let port = port.unwrap_or(22);
    if port == 0 {
        return Err("端口必须在 1–65535 之间".into());
    }
    let normalized = if let Some(ipv6) = parse_ipv6(host) {
        ipv6.to_string()
    } else if let Ok(ipv4) = host.parse::<Ipv4Addr>() {
        ipv4.to_string()
    } else {
        normalize_dns(host)?
    };
    Ok((normalized, port))
}

fn parse_ipv6(host: &str) -> Option<Ipv6Addr> {
    let bare = host.strip_prefix('[').and_then(|value| value.strip_suffix(']')).unwrap_or(host);
    if bare.contains('%') {
        return None;
    }
    bare.parse().ok()
}

fn normalize_dns(host: &str) -> Result<String, String> {
    if host.parse::<Ipv6Addr>().is_ok() {
        return Err("主机名无效".into());
    }
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return Err("主机名无效".into());
    }
    for label in host.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err("主机名无效".into());
        }
    }
    Ok(host)
}

pub fn validate_ssh_connection(
    name: &str,
    host: &str,
    port: Option<u16>,
    username: &str,
    auth_type: &str,
    private_key_path: Option<&str>,
) -> Result<SshConnectionInput, String> {
    let name = validate_name(name)?.to_owned();
    let (host, port) = normalize_endpoint(host, port)?;
    let username = username.trim();
    if username.is_empty()
        || username.chars().count() > 128
        || username.contains(['\0', ' ', '\n', '\r'])
    {
        return Err("用户名需要包含 1–128 个字符，且不能包含空白".into());
    }
    let auth_type = SshAuthType::parse(auth_type)?;
    let private_key_path = match auth_type {
        SshAuthType::Password => {
            if private_key_path.is_some_and(|path| !path.is_empty()) {
                return Err("密码认证不能保存私钥路径".into());
            }
            None
        }
        SshAuthType::PrivateKey => {
            let path = private_key_path.unwrap_or("").trim();
            if path.is_empty() || path.len() > 1024 || path.contains('\0') {
                return Err("私钥认证需要本机私钥路径".into());
            }
            Some(path.to_owned())
        }
    };
    Ok(SshConnectionInput {
        name,
        host,
        port,
        username: username.to_owned(),
        auth_type,
        private_key_path,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteLayout {
    pub schema_version: u32,
    pub active_tab_id: Option<String>,
    pub tabs: Vec<RemoteTab>,
}

impl Default for RemoteLayout {
    fn default() -> Self {
        Self { schema_version: 1, active_tab_id: None, tabs: Vec::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RemoteTab {
    pub id: String,
    pub title: String,
    pub root: RemoteNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum RemoteNode {
    Terminal {
        #[serde(rename = "terminalConfigId")]
        terminal_config_id: String,
    },
    Split {
        direction: Direction,
        ratio: f64,
        children: Vec<RemoteNode>,
    },
}

impl RemoteLayout {
    pub fn terminal_ids(&self) -> HashSet<String> {
        let mut ids = HashSet::new();
        for tab in &self.tabs {
            collect_ids(&tab.root, &mut ids);
        }
        ids
    }

    pub fn validate(&self, configs: &HashSet<String>) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("不支持的远程布局版本".into());
        }
        if serde_json::to_vec(self).map(|bytes| bytes.len() > MAX_REMOTE_LAYOUT_BYTES).unwrap_or(true) {
            return Err("远程布局不能超过 256 KiB".into());
        }
        let mut seen_tabs = HashSet::new();
        let mut seen_terminals = HashSet::new();
        for tab in &self.tabs {
            if tab.id.is_empty() || !seen_tabs.insert(tab.id.clone()) {
                return Err("Tab ID 不能为空或重复".into());
            }
            validate_name(&tab.title)?;
            validate_node(&tab.root, configs, &mut seen_terminals, 0)?;
        }
        if self.active_tab_id.as_ref().is_some_and(|id| self.tabs.iter().any(|tab| &tab.id == id))
            || (self.tabs.is_empty() && self.active_tab_id.is_none())
        {
            Ok(())
        } else {
            Err("激活 Tab 必须指向现有 Tab".into())
        }
    }

    /// Drop editor nodes, unknown terminals, and broken structure. The caller
    /// keeps the connection row even when the layout becomes empty.
    pub fn repair(raw: &str, configs: &HashSet<String>) -> Self {
        if raw.len() > MAX_REMOTE_LAYOUT_BYTES {
            return Self::default();
        }
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            return Self::default();
        };
        let mut layout = Self {
            schema_version: value.get("schemaVersion").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            active_tab_id: value.get("activeTabId").and_then(|v| v.as_str()).map(str::to_owned),
            tabs: Vec::new(),
        };
        if layout.schema_version != 1 {
            return Self::default();
        }
        let mut seen_terminals = HashSet::new();
        for tab in value.get("tabs").and_then(|v| v.as_array()).into_iter().flatten() {
            let Some(id) = tab.get("id").and_then(|v| v.as_str()).filter(|id| !id.is_empty()) else {
                continue;
            };
            if layout.tabs.iter().any(|existing| existing.id == id) {
                continue;
            }
            let title = tab.get("title").and_then(|v| v.as_str()).unwrap_or("Terminal");
            let Ok(title) = validate_name(title) else { continue };
            let Some(root) = tab.get("root").and_then(|node| repair_node(node, configs, &mut seen_terminals, 0)) else {
                continue;
            };
            layout.tabs.push(RemoteTab { id: id.to_owned(), title: title.to_owned(), root });
        }
        if !layout.active_tab_id.as_ref().is_some_and(|id| layout.tabs.iter().any(|tab| &tab.id == id)) {
            layout.active_tab_id = layout.tabs.first().map(|tab| tab.id.clone());
        }
        if layout.validate(configs).is_err() { Self::default() } else { layout }
    }
}

fn collect_ids(node: &RemoteNode, ids: &mut HashSet<String>) {
    match node {
        RemoteNode::Terminal { terminal_config_id } => {
            ids.insert(terminal_config_id.clone());
        }
        RemoteNode::Split { children, .. } => {
            for child in children {
                collect_ids(child, ids);
            }
        }
    }
}

fn validate_node(
    node: &RemoteNode,
    configs: &HashSet<String>,
    seen: &mut HashSet<String>,
    depth: usize,
) -> Result<(), String> {
    match node {
        RemoteNode::Terminal { terminal_config_id } => {
            if !configs.contains(terminal_config_id) || !seen.insert(terminal_config_id.clone()) {
                return Err("终端配置不属于此远程工作区，或在布局中重复引用".into());
            }
            Ok(())
        }
        RemoteNode::Split { ratio, children, .. } => {
            if depth >= 8 || children.len() != 2 || !ratio.is_finite() || *ratio <= 0.0 || *ratio >= 1.0 {
                return Err("分屏需要两个子节点，比例在 (0, 1) 内，嵌套最多 8 层".into());
            }
            for child in children {
                validate_node(child, configs, seen, depth + 1)?;
            }
            Ok(())
        }
    }
}

fn repair_node(node: &Value, configs: &HashSet<String>, seen: &mut HashSet<String>, depth: usize) -> Option<RemoteNode> {
    let kind = node.get("type").and_then(|v| v.as_str())?;
    match kind {
        "terminal" => {
            let id = node.get("terminalConfigId").and_then(|v| v.as_str())?;
            if !configs.contains(id) || !seen.insert(id.to_owned()) {
                return None;
            }
            Some(RemoteNode::Terminal { terminal_config_id: id.to_owned() })
        }
        "split" => {
            if depth >= 8 {
                return None;
            }
            let direction = match node.get("direction").and_then(|v| v.as_str()) {
                Some("horizontal") => Direction::Horizontal,
                Some("vertical") => Direction::Vertical,
                _ => return None,
            };
            let ratio = node.get("ratio").and_then(|v| v.as_f64())?;
            if !ratio.is_finite() || ratio <= 0.0 || ratio >= 1.0 {
                return None;
            }
            let children = node.get("children").and_then(|v| v.as_array())?;
            let children: Vec<_> = children.iter().filter_map(|child| repair_node(child, configs, seen, depth + 1)).collect();
            match children.len() {
                0 => None,
                1 => children.into_iter().next(),
                _ => Some(RemoteNode::Split { direction, ratio, children: vec![children[0].clone(), children[1].clone()] }),
            }
        }
        _ => None,
    }
}

pub fn empty_remote_layout() -> Value {
    json!({"schemaVersion": 1, "activeTabId": null, "tabs": []})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_dns_case_and_equivalent_ipv6() {
        assert_eq!(normalize_endpoint("Example.COM", None).unwrap(), ("example.com".into(), 22));
        assert_eq!(normalize_endpoint("::1", Some(2222)).unwrap().0, "::1");
        assert_eq!(normalize_endpoint("[::1]", Some(22)).unwrap().0, normalize_endpoint("0:0:0:0:0:0:0:1", Some(22)).unwrap().0);
        assert_eq!(normalize_endpoint("127.0.0.1", Some(22)).unwrap().0, "127.0.0.1");
        for host in ["user@host", "ssh://host", "-oProxyJump=x", "host/path", "", "a b"] {
            assert!(normalize_endpoint(host, Some(22)).is_err(), "{host}");
        }
        assert!(normalize_endpoint("example.com", Some(0)).is_err());
    }

    #[test]
    fn accepts_two_users_and_rejects_secrets_in_the_model() {
        let password = validate_ssh_connection("lab", "Example.COM", None, "root", "password", None).unwrap();
        assert_eq!(password.port, 22);
        assert!(password.private_key_path.is_none());
        let key = validate_ssh_connection("lab", "example.com", Some(22), "dev", "privateKey", Some("C:/keys/id_ed25519")).unwrap();
        assert_eq!(key.username, "dev");
        assert!(validate_ssh_connection("lab", "example.com", Some(22), "dev", "privateKey", None).is_err());
        assert!(validate_ssh_connection("lab", "example.com", Some(22), "dev", "password", Some("C:/keys/id_ed25519")).is_err());
    }

    #[test]
    fn repairs_remote_layout_without_requiring_a_connection_delete() {
        let configs = HashSet::from(["term".into()]);
        let raw = r#"{"schemaVersion":1,"activeTabId":"tab","tabs":[{"id":"tab","title":"Shell","root":{"type":"split","direction":"horizontal","ratio":0.5,"children":[{"type":"editor","id":"e","path":"a.rs"},{"type":"terminal","terminalConfigId":"term"}]}}]}"#;
        let repaired = RemoteLayout::repair(raw, &configs);
        assert_eq!(repaired.tabs.len(), 1);
        assert_eq!(repaired.tabs[0].root, RemoteNode::Terminal { terminal_config_id: "term".into() });
        assert!(RemoteLayout::repair("not-json", &configs).tabs.is_empty());
        let mut foreign = repaired.clone();
        foreign.tabs[0].root = RemoteNode::Terminal { terminal_config_id: "other".into() };
        assert!(foreign.validate(&configs).is_err());
    }
}
