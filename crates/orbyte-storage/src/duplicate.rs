//! Transactional project duplication.
//! Author: jaswtao
//! Date: 2026-11-02

use super::{db_error, layout, Database, Result};
use orbyte_core::{validate_name, ProjectInput, RemoteLayout};
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

fn new_id() -> String { Uuid::new_v4().to_string() }

fn remap_node(node: &mut Value, terminals: &HashMap<String, String>) -> Result<()> {
    match node.get("type").and_then(Value::as_str) {
        Some("terminal") => {
            let old = node.get("terminalConfigId").and_then(Value::as_str).ok_or("Missing terminal ID")?;
            node["terminalConfigId"] = json!(terminals.get(old).ok_or("Missing copied terminal")?);
        }
        Some("editor") => node["id"] = json!(new_id()),
        Some("split") => for child in node["children"].as_array_mut().ok_or("Invalid split")? { remap_node(child, terminals)?; },
        _ => return Err("Invalid layout node".into()),
    }
    Ok(())
}

fn remap_layout(mut value: Value, terminals: &HashMap<String, String>) -> Result<String> {
    let active = value.get("activeTabId").and_then(Value::as_str).map(str::to_owned);
    let tabs = value["tabs"].as_array_mut().ok_or("Invalid layout tabs")?;
    let mut new_active = None;
    for tab in tabs {
        let old = tab["id"].as_str().ok_or("Missing tab ID")?;
        let id = new_id();
        if active.as_deref() == Some(old) { new_active = Some(id.clone()); }
        tab["id"] = json!(id);
        remap_node(&mut tab["root"], terminals)?;
    }
    value["activeTabId"] = json!(new_active);
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

impl Database {
    pub async fn duplicate_project(&self, source: &str, input: ProjectInput) -> Result<String> {
        let name = validate_name(&input.name)?;
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let workspace: String = sqlx::query_scalar("SELECT id FROM t_workspace WHERE project_id = ?")
            .bind(source).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or("Project no longer exists")?;
        let layout = layout::read_repaired(&mut tx, &workspace).await?;
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM t_terminal_config WHERE workspace_id = ?")
            .bind(&workspace).fetch_all(&mut *tx).await.map_err(db_error)?;
        let mapping: HashMap<String, String> = ids.into_iter().map(|id| (id, new_id())).collect();
        let mapped = remap_layout(serde_json::to_value(layout).map_err(|e| e.to_string())?, &mapping)?;
        let project = new_id();
        let new_workspace = new_id();
        sqlx::query("INSERT INTO t_project(id, group_id, name, path, favorite, sort_order) VALUES (?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM t_project))")
            .bind(&project).bind(input.group_id).bind(name).bind(input.path).bind(input.favorite).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("INSERT INTO t_workspace(id, project_id) VALUES (?, ?)").bind(&new_workspace).bind(&project).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("INSERT INTO t_terminal_config(id, workspace_id, name, shell_type, cwd, args_json, launch_preset) SELECT m.value, ?, c.name, c.shell_type, c.cwd, c.args_json, c.launch_preset FROM t_terminal_config c JOIN json_each(?) m ON m.key = c.id WHERE c.workspace_id = ?")
            .bind(&new_workspace).bind(json!(mapping).to_string()).bind(&workspace).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("INSERT INTO t_workspace_layout(id, workspace_id, schema_version, layout_json) VALUES (?, ?, 1, ?)")
            .bind(new_id()).bind(&new_workspace).bind(mapped).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(project)
    }

    pub async fn duplicate_remote_project(&self, source: &str, name: &str) -> Result<String> {
        let name = validate_name(name)?;
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let raw: String = sqlx::query_scalar("SELECT layout_json FROM t_remote_project WHERE id = ?")
            .bind(source).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or("Remote project no longer exists")?;
        let workspaces: Vec<String> = sqlx::query_scalar("SELECT id FROM t_remote_workspace WHERE project_id = ?")
            .bind(source).fetch_all(&mut *tx).await.map_err(db_error)?;
        let rows = sqlx::query("SELECT c.id FROM t_remote_terminal_config c JOIN t_remote_workspace w ON w.id = c.remote_workspace_id WHERE w.project_id = ?")
            .bind(source).fetch_all(&mut *tx).await.map_err(db_error)?;
        let ids: HashSet<String> = rows.into_iter().map(|row| row.get("id")).collect();
        let layout = RemoteLayout::repair(&raw, &ids);
        let terminals: HashMap<String, String> = ids.into_iter().map(|id| (id, new_id())).collect();
        let contexts: HashMap<String, String> = workspaces.into_iter().map(|id| (id, new_id())).collect();
        let mapped = remap_layout(serde_json::to_value(layout).map_err(|e| e.to_string())?, &terminals)?;
        let project = new_id();
        sqlx::query("INSERT INTO t_remote_project(id, name, layout_json, sort_order) VALUES (?, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM t_remote_project))")
            .bind(&project).bind(name).bind(mapped).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("INSERT INTO t_remote_workspace(id, connection_id, project_id, schema_version, layout_json, last_directory) SELECT m.value, w.connection_id, ?, 1, ?, w.last_directory FROM t_remote_workspace w JOIN json_each(?) m ON m.key = w.id WHERE w.project_id = ?")
            .bind(&project).bind(orbyte_core::empty_remote_layout().to_string()).bind(json!(contexts).to_string()).bind(source).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("INSERT INTO t_remote_terminal_config(id, remote_workspace_id, name, sort_order) SELECT t.value, w.value, c.name, c.sort_order FROM t_remote_terminal_config c JOIN json_each(?) t ON t.key = c.id JOIN json_each(?) w ON w.key = c.remote_workspace_id")
            .bind(json!(terminals).to_string()).bind(json!(contexts).to_string()).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn input(path: &str) -> ProjectInput {
        ProjectInput { name: "Copy".into(), path: path.into(), group_id: None, favorite: true }
    }

    #[tokio::test]
    async fn copies_local_layout_with_shared_path_and_rolls_back_invalid_group() {
        let db = Database::open(Path::new(":memory:")).await.unwrap();
        let source = db.create_project(input("C:/source")).await.unwrap();
        let workspace = db.list_projects().await.unwrap()[0].workspace_id.clone();
        let terminal = db.create_terminal_config(&source, &workspace, "Shell", "cmd", "src").await.unwrap();
        let raw = json!({"schemaVersion":1,"activeTabId":"tab","tabs":[{"id":"tab","title":"Split","root":{"type":"split","direction":"horizontal","ratio":0.5,"children":[{"type":"terminal","terminalConfigId":terminal.id},{"type":"editor","id":"editor","path":"src/file.txt"}]}}]}).to_string();
        db.save_workspace_layout(&workspace, &raw).await.unwrap();
        let mut invalid = input("C:/source");
        invalid.group_id = Some("missing".into());
        assert!(db.duplicate_project(&source, invalid).await.is_err());
        assert!(db.duplicate_project("missing", input("C:/other")).await.is_err());
        assert_eq!(db.list_projects().await.unwrap().len(), 1);
        let copy = db.duplicate_project(&source, input("C:/source")).await.unwrap();
        let copy_workspace = db.list_projects().await.unwrap().into_iter().find(|p| p.id == copy).unwrap().workspace_id;
        let configs = db.list_terminal_configs(&copy_workspace).await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_ne!(configs[0].id, terminal.id);
        assert_eq!(configs[0].cwd, "src");
        let layout = db.load_workspace_layout(&copy_workspace).await.unwrap();
        assert_eq!(layout.terminal_ids(), HashSet::from([configs[0].id.clone()]));
        assert_ne!(layout.tabs[0].id, "tab");
        db.delete_project(&copy).await.unwrap();
        assert_eq!(db.list_terminal_configs(&workspace).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn copies_remote_contexts_and_layout_with_independent_ids() {
        let db = Database::open(Path::new(":memory:")).await.unwrap();
        let first = db.create_ssh_connection(super::super::parse_connection("one", "one.example", None, "a", "password", None).unwrap()).await.unwrap();
        let second = db.create_ssh_connection(super::super::parse_connection("two", "two.example", None, "b", "password", None).unwrap()).await.unwrap();
        let source = db.save_remote_project(None, "Source").await.unwrap();
        let a = db.create_remote_project_terminal(&source, &first, "One").await.unwrap();
        let b = db.create_remote_project_terminal(&source, &second, "Two").await.unwrap();
        sqlx::query("UPDATE t_remote_workspace SET last_directory = '/home' WHERE id = ?").bind(&a.remote_workspace_id).execute(&db.pool).await.unwrap();
        let layout = serde_json::from_value(json!({"schemaVersion":1,"activeTabId":"tab","tabs":[{"id":"tab","title":"Both","root":{"type":"split","direction":"vertical","ratio":0.5,"children":[{"type":"terminal","terminalConfigId":a.id},{"type":"terminal","terminalConfigId":b.id}]}}]})).unwrap();
        db.save_remote_project_layout(&source, &layout).await.unwrap();
        let copy = db.duplicate_remote_project(&source, "Copy").await.unwrap();
        let loaded = db.load_remote_project_workspace(&copy).await.unwrap();
        assert_eq!(loaded.contexts.len(), 2);
        assert!(loaded.contexts.iter().any(|c| c.connection_id == first && c.last_directory.as_deref() == Some("/home")));
        assert_eq!(loaded.terminals.len(), 2);
        assert!(loaded.terminals.iter().all(|t| t.id != a.id && t.id != b.id));
        assert_eq!(loaded.layout.terminal_ids(), loaded.terminals.iter().map(|t| t.id.clone()).collect());
        assert_ne!(loaded.layout.tabs[0].id, "tab");
        db.delete_remote_project(&copy).await.unwrap();
        assert_eq!(db.load_remote_project_workspace(&source).await.unwrap().terminals.len(), 2);
        let empty = db.save_remote_project(None, "Empty").await.unwrap();
        let empty_copy = db.duplicate_remote_project(&empty, "Empty copy").await.unwrap();
        assert!(db.load_remote_project_workspace(&empty_copy).await.unwrap().layout.tabs.is_empty());
        assert!(db.duplicate_remote_project("missing", "Copy").await.is_err());
    }
}
