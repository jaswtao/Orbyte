//! 终端配置持久化；运行时会话不进入数据库。
//! Author: jaswtao
//! Date: 2026-10-13

use super::{db_error, require_changed, Database, Result};
use orbyte_core::{validate_name, validate_relative_path, validate_shell, TerminalConfig};
use sqlx::Row;
use uuid::Uuid;

fn config(row: &sqlx::sqlite::SqliteRow) -> TerminalConfig {
    TerminalConfig {
        id: row.get("id"),
        workspace_id: row.get("workspace_id"),
        name: row.get("name"),
        shell_type: row.get("shell_type"),
        cwd: row.get("cwd"),
        launch_preset: row.get("launch_preset"),
    }
}

impl Database {
    pub async fn list_project_terminal_configs(
        &self,
        project_id: &str,
    ) -> Result<Vec<TerminalConfig>> {
        let rows = sqlx::query("SELECT c.* FROM t_terminal_config c JOIN t_workspace w ON w.id = c.workspace_id WHERE w.project_id = ? ORDER BY c.created_at, c.id")
            .bind(project_id).fetch_all(&self.pool).await.map_err(db_error)?;
        Ok(rows.iter().map(config).collect())
    }

    pub async fn workspaces_of_configs(&self, config_ids: &[String]) -> Result<Vec<String>> {
        let ids = serde_json::to_string(config_ids).map_err(|e| e.to_string())?;
        sqlx::query_scalar("SELECT DISTINCT workspace_id FROM t_terminal_config WHERE id IN (SELECT value FROM json_each(?))")
            .bind(ids).fetch_all(&self.pool).await.map_err(db_error)
    }

    pub async fn workspace_project_path(
        &self,
        project_id: &str,
        workspace_id: &str,
    ) -> Result<String> {
        sqlx::query_scalar("SELECT p.path FROM t_project p JOIN t_workspace w ON w.project_id = p.id WHERE p.id = ? AND w.id = ?")
            .bind(project_id).bind(workspace_id).fetch_optional(&self.pool).await.map_err(db_error)?
            .ok_or_else(|| "项目或工作区不存在，或工作区不属于该项目".into())
    }

    pub async fn create_terminal_config(&self, project_id: &str, workspace_id: &str, name: &str, shell: &str, cwd: &str) -> Result<TerminalConfig> {
        self.create_terminal_config_with_preset(project_id, workspace_id, name, shell, cwd, None).await
    }

    pub async fn create_terminal_config_with_preset(
        &self,
        project_id: &str,
        workspace_id: &str,
        name: &str,
        shell: &str,
        cwd: &str,
        launch_preset: Option<&str>,
    ) -> Result<TerminalConfig> {
        orbyte_core::validate_launch_preset(launch_preset)?;
        let name = validate_name(name)?;
        validate_shell(shell)?;
        validate_relative_path(cwd)?;
        let id = Uuid::new_v4().to_string();
        let result = sqlx::query("INSERT INTO t_terminal_config(id, workspace_id, name, shell_type, cwd, launch_preset) SELECT ?, w.id, ?, ?, ?, ? FROM t_workspace w WHERE w.id = ? AND w.project_id = ?")
            .bind(&id).bind(name).bind(shell).bind(cwd).bind(launch_preset).bind(workspace_id).bind(project_id)
            .execute(&self.pool).await.map_err(db_error)?;
        require_changed(result.rows_affected())?;
        Ok(TerminalConfig {
            id,
            workspace_id: workspace_id.into(),
            name: name.into(),
            shell_type: shell.into(),
            cwd: cwd.into(),
            launch_preset: launch_preset.map(str::to_owned),
        })
    }

    pub async fn list_terminal_configs(&self, workspace_id: &str) -> Result<Vec<TerminalConfig>> {
        let rows = sqlx::query(
            "SELECT * FROM t_terminal_config WHERE workspace_id = ? ORDER BY created_at, id",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(rows.iter().map(config).collect())
    }

    pub async fn terminal_config_with_path(&self, id: &str) -> Result<(TerminalConfig, String)> {
        let row = sqlx::query("SELECT c.*, p.path AS project_path FROM t_terminal_config c JOIN t_workspace w ON w.id = c.workspace_id JOIN t_project p ON p.id = w.project_id WHERE c.id = ?")
            .bind(id).fetch_optional(&self.pool).await.map_err(db_error)?
            .ok_or("终端配置不存在或已被移除")?;
        Ok((config(&row), row.get("project_path")))
    }

    pub async fn delete_terminal_config(&self, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM t_terminal_config WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        require_changed(result.rows_affected())
    }
}
