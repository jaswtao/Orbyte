//! 布局保存与加载修正在事务中完成，批量清理孤立终端配置。
//! Author: jaswtao
//! Date: 2026-10-13

use super::{db_error, Database, Result};
use orbyte_core::layout::Layout;
use sqlx::{Sqlite, Transaction};
use std::collections::HashSet;
use uuid::Uuid;

async fn configs(tx: &mut Transaction<'_, Sqlite>, workspace_id: &str) -> Result<HashSet<String>> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM t_workspace WHERE id = ?)")
        .bind(workspace_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
    if !exists {
        return Err("工作区不存在".into());
    }
    Ok(
        sqlx::query_scalar::<_, String>("SELECT id FROM t_terminal_config WHERE workspace_id = ?")
            .bind(workspace_id)
            .fetch_all(&mut **tx)
            .await
            .map_err(db_error)?
            .into_iter()
            .collect(),
    )
}

async fn persist(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: &str,
    layout: &Layout,
) -> Result<()> {
    sqlx::query("INSERT INTO t_workspace_layout(id, workspace_id, schema_version, layout_json) VALUES (?, ?, 1, ?) ON CONFLICT(workspace_id) DO UPDATE SET schema_version = 1, layout_json = excluded.layout_json, updated_at = CURRENT_TIMESTAMP")
        .bind(Uuid::new_v4().to_string()).bind(workspace_id)
        .bind(serde_json::to_string(layout).map_err(|e| e.to_string())?)
        .execute(&mut **tx).await.map_err(db_error)?;
    Ok(())
}

async fn read_repaired(tx: &mut Transaction<'_, Sqlite>, workspace_id: &str) -> Result<Layout> {
    let ids = configs(tx, workspace_id).await?;
    let raw: Option<String> =
        sqlx::query_scalar("SELECT layout_json FROM t_workspace_layout WHERE workspace_id = ?")
            .bind(workspace_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_error)?;
    match raw {
        Some(raw) => Layout::parse(&raw)?,
        None => Layout::default(),
    }
    .repair(&ids)
}

impl Database {
    /// 命令层先预览引用，再结束孤立会话；成功后才执行配置清理。
    pub async fn preview_workspace_layout(&self, workspace_id: &str) -> Result<Layout> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let layout = read_repaired(&mut tx, workspace_id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(layout)
    }
    pub async fn save_workspace_layout(&self, workspace_id: &str, raw: &str) -> Result<()> {
        let layout = Layout::parse(raw)?;
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        layout.validate(&configs(&mut tx, workspace_id).await?)?;
        persist(&mut tx, workspace_id, &layout).await?;
        tx.commit().await.map_err(db_error)
    }

    pub async fn load_workspace_layout(&self, workspace_id: &str) -> Result<Layout> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let layout = read_repaired(&mut tx, workspace_id).await?;
        let referenced: Vec<_> = layout.terminal_ids().into_iter().collect();
        // 单条 DELETE，避免在遍历布局时执行 CRUD。
        sqlx::query("DELETE FROM t_terminal_config WHERE workspace_id = ? AND id NOT IN (SELECT value FROM json_each(?))")
            .bind(workspace_id).bind(serde_json::to_string(&referenced).map_err(|e| e.to_string())?)
            .execute(&mut *tx).await.map_err(db_error)?;
        persist(&mut tx, workspace_id, &layout).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbyte_core::ProjectInput;
    use serde_json::json;

    #[tokio::test]
    async fn restores_layout_repairs_references_and_removes_orphans() {
        let path = std::env::temp_dir().join(format!("orbyte-layout-{}.db", Uuid::new_v4()));
        let db = Database::open(&path).await.unwrap();
        let project = db
            .create_project(ProjectInput {
                name: "布局".into(),
                path: "C:/layout".into(),
                group_id: None,
                favorite: false,
            })
            .await
            .unwrap();
        let workspace = db.list_projects().await.unwrap()[0].workspace_id.clone();
        let kept = db
            .create_terminal_config(&project, &workspace, "cmd", "cmd", "")
            .await
            .unwrap();
        let removed = db
            .create_terminal_config(&project, &workspace, "cmd", "cmd", "")
            .await
            .unwrap();
        let raw = json!({"schemaVersion":1,"activeTabId":"t","tabs":[{"id":"t","title":"分屏","root":{"type":"split","direction":"horizontal","ratio":0.37,"children":[{"type":"terminal","terminalConfigId":kept.id},{"type":"terminal","terminalConfigId":removed.id}]}}]}).to_string();
        db.save_workspace_layout(&workspace, &raw).await.unwrap();
        let expected = db.load_workspace_layout(&workspace).await.unwrap();
        db.pool.close().await;
        let db = Database::open(&path).await.unwrap();
        assert_eq!(
            db.load_workspace_layout(&workspace).await.unwrap(),
            expected
        );
        db.delete_terminal_config(&removed.id).await.unwrap();
        db.create_terminal_config(&project, &workspace, "孤立", "cmd", "")
            .await
            .unwrap();
        let fixed = db.load_workspace_layout(&workspace).await.unwrap();
        assert_eq!(fixed.terminal_ids(), HashSet::from([kept.id.clone()]));
        assert_eq!(db.list_terminal_configs(&workspace).await.unwrap().len(), 1);
        assert_eq!(db.load_workspace_layout(&workspace).await.unwrap(), fixed);
        assert!(db.save_workspace_layout(&workspace, &raw).await.is_err());
        assert_eq!(db.load_workspace_layout(&workspace).await.unwrap(), fixed);
        assert!(db.load_workspace_layout("missing").await.is_err());
        db.pool.close().await;
        std::fs::remove_file(path).unwrap();
    }
}
