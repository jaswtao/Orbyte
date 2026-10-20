//! 工作区布局校验与崩溃后修正。
//! Author: jaswtao
//! Date: 2026-10-13

use crate::{validate_name, validate_relative_path};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_LAYOUT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Layout {
    pub schema_version: u32,
    pub active_tab_id: Option<String>,
    pub tabs: Vec<Tab>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            schema_version: 1,
            active_tab_id: None,
            tabs: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tab {
    pub id: String,
    pub title: String,
    pub root: Node,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Node {
    Terminal {
        #[serde(rename = "terminalConfigId")]
        terminal_config_id: String,
    },
    Editor {
        id: String,
        path: String,
    },
    Split {
        direction: Direction,
        ratio: f64,
        children: Vec<Node>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Horizontal,
    Vertical,
}

#[derive(Default)]
struct References {
    terminals: HashSet<String>,
    editors: HashSet<String>,
    paths: HashSet<String>,
}

impl Layout {
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() > MAX_LAYOUT_BYTES {
            return Err("布局不能超过 256 KiB".into());
        }
        serde_json::from_str(raw).map_err(|e| format!("无法读取工作区布局：{e}"))
    }

    pub fn terminal_ids(&self) -> HashSet<String> {
        fn collect(node: &Node, ids: &mut HashSet<String>) {
            match node {
                Node::Terminal { terminal_config_id } => {
                    ids.insert(terminal_config_id.clone());
                }
                Node::Split { children, .. } => {
                    for child in children {
                        collect(child, ids);
                    }
                }
                Node::Editor { .. } => {}
            }
        }
        let mut ids = HashSet::new();
        for tab in &self.tabs {
            collect(&tab.root, &mut ids);
        }
        ids
    }

    pub fn validate(&self, configs: &HashSet<String>) -> Result<(), String> {
        self.check_structure()?;
        let mut refs = References::default();
        for tab in &self.tabs {
            validate_node(&tab.root, configs, &mut refs)?;
        }
        if self
            .active_tab_id
            .as_ref()
            .is_some_and(|id| self.tabs.iter().any(|t| &t.id == id))
            || (self.tabs.is_empty() && self.active_tab_id.is_none())
        {
            Ok(())
        } else {
            Err("激活 Tab 必须指向现有 Tab".into())
        }
    }

    fn check_structure(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("不支持的布局版本".into());
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_LAYOUT_BYTES {
            return Err("布局不能超过 256 KiB".into());
        }
        let mut tabs = HashSet::new();
        for tab in &self.tabs {
            if tab.id.is_empty() || !tabs.insert(&tab.id) {
                return Err("Tab ID 不能为空或重复".into());
            }
            validate_name(&tab.title)?;
            check_node(&tab.root, 0)?;
        }
        Ok(())
    }

    /// 不检查文件存在性；仅移除失效引用和不安全路径，再折叠分屏。
    pub fn repair(mut self, configs: &HashSet<String>) -> Result<Self, String> {
        self.check_structure()?;
        let mut refs = References::default();
        self.tabs = self
            .tabs
            .into_iter()
            .filter_map(|tab| {
                repair_node(tab.root, configs, &mut refs).map(|root| Tab { root, ..tab })
            })
            .collect();
        if !self
            .active_tab_id
            .as_ref()
            .is_some_and(|id| self.tabs.iter().any(|t| &t.id == id))
        {
            self.active_tab_id = self.tabs.first().map(|t| t.id.clone());
        }
        self.validate(configs)?;
        Ok(self)
    }
}

fn check_node(node: &Node, depth: usize) -> Result<(), String> {
    if let Node::Split {
        ratio, children, ..
    } = node
    {
        if depth >= 8 || children.len() != 2 || !ratio.is_finite() || *ratio <= 0.0 || *ratio >= 1.0
        {
            return Err("分屏需要两个子节点，比例在 (0, 1) 内，嵌套最多 8 层".into());
        }
        for child in children {
            check_node(child, depth + 1)?;
        }
    }
    Ok(())
}

fn validate_node(
    node: &Node,
    configs: &HashSet<String>,
    refs: &mut References,
) -> Result<(), String> {
    match node {
        Node::Terminal { terminal_config_id } => {
            if !configs.contains(terminal_config_id)
                || !refs.terminals.insert(terminal_config_id.clone())
            {
                return Err("终端配置不属于此工作区，或在布局中重复引用".into());
            }
        }
        Node::Editor { id, path } => {
            validate_relative_path(path)?;
            if path.is_empty()
                || id.is_empty()
                || refs.editors.contains(id)
                || refs.paths.contains(&path.to_lowercase())
            {
                return Err("编辑器 ID 和文件路径不能为空或重复".into());
            }
            refs.editors.insert(id.clone());
            refs.paths.insert(path.to_lowercase());
        }
        Node::Split { children, .. } => {
            for child in children {
                validate_node(child, configs, refs)?;
            }
        }
    }
    Ok(())
}

fn repair_node(node: Node, configs: &HashSet<String>, refs: &mut References) -> Option<Node> {
    match node {
        Node::Split {
            direction,
            ratio,
            children,
        } => {
            let mut children: Vec<_> = children
                .into_iter()
                .filter_map(|n| repair_node(n, configs, refs))
                .collect();
            match children.len() {
                0 => None,
                1 => children.pop(),
                _ => Some(Node::Split {
                    direction,
                    ratio,
                    children,
                }),
            }
        }
        leaf => validate_node(&leaf, configs, refs).ok().map(|_| leaf),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn layout(root: serde_json::Value) -> Layout {
        Layout::parse(&json!({"schemaVersion":1,"activeTabId":"tab","tabs":[{"id":"tab","title":"终端","root":root}]}).to_string()).unwrap()
    }
    fn terminal(id: &str) -> serde_json::Value {
        json!({"type":"terminal","terminalConfigId":id})
    }

    #[test]
    fn validates_ownership_duplicates_and_editor_paths() {
        let configs = HashSet::from(["a".into()]);
        layout(terminal("a")).validate(&configs).unwrap();
        assert!(layout(terminal("other")).validate(&configs).is_err());
        assert!(layout(json!({"type":"split","direction":"horizontal","ratio":0.5,"children":[terminal("a"),terminal("a")]})).validate(&configs).is_err());
        for path in ["", "../outside", "NUL.txt"] {
            assert!(layout(json!({"type":"editor","id":"e","path":path}))
                .validate(&configs)
                .is_err());
        }
        // 尚不存在的文件也能持久化。
        layout(json!({"type":"editor","id":"e","path":"src/missing.rs"}))
            .validate(&configs)
            .unwrap();
        let editor = |id| json!({"type":"editor","id":id,"path":"Src/Main.rs"});
        assert!(layout(json!({"type":"split","direction":"vertical","ratio":0.5,"children":[editor("e1"),json!({"type":"editor","id":"e2","path":"src/main.rs"})]})).validate(&configs).is_err());
    }

    #[test]
    fn repairs_missing_leaves_collapses_splits_and_selects_first_tab() {
        let configs = HashSet::from(["a".into()]);
        let mut value = layout(
            json!({"type":"split","direction":"vertical","ratio":0.3,"children":[terminal("gone"),terminal("a")]}),
        );
        value.active_tab_id = Some("gone".into());
        let repaired = value.repair(&configs).unwrap();
        assert_eq!(
            repaired.tabs[0].root,
            Node::Terminal {
                terminal_config_id: "a".into()
            }
        );
        assert_eq!(repaired.active_tab_id.as_deref(), Some("tab"));
        assert_eq!(
            layout(json!({"type":"editor","id":"e","path":"../outside"}))
                .repair(&configs)
                .unwrap(),
            Layout::default()
        );
    }

    #[test]
    fn rejects_bad_structure_version_size_and_depth() {
        let configs = HashSet::from(["a".into()]);
        for ratio in [0.0, 1.0, -0.1] {
            assert!(layout(json!({"type":"split","direction":"horizontal","ratio":ratio,"children":[terminal("a"),terminal("b")]})).repair(&configs).is_err());
        }
        let mut root = terminal("a");
        for _ in 0..9 {
            root = json!({"type":"split","direction":"horizontal","ratio":0.5,"children":[root,terminal("b")]});
        }
        assert!(layout(root).repair(&configs).is_err());
        let mut value = Layout::default();
        value.schema_version = 2;
        assert!(value.repair(&configs).is_err());
        assert!(Layout::parse(&" ".repeat(MAX_LAYOUT_BYTES + 1)).is_err());
    }
}
