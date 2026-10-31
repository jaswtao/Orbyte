//! SSH connection, known-host, and remote workspace persistence.
//!
//! Passwords and key passphrases are not columns. Deleting a connection cascades
//! only to its remote workspace and terminal configs.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use orbyte_core::{
    empty_remote_layout, validate_ssh_connection, RemoteLayout, SshConnectionInput,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::HashSet;
use uuid::Uuid;

use super::{db_error, require_changed, Database, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnection {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: String,
    pub private_key_path: Option<String>,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownHost {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint_sha256: String,
}

#[derive(Debug, Clone)]
pub struct KnownHostRecord {
    pub algorithm: String,
    pub public_key: Vec<u8>,
    pub fingerprint_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWorkspaceRecord {
    pub id: String,
    pub connection_id: String,
    pub layout: RemoteLayout,
    pub last_directory: Option<String>,
    pub terminals: Vec<RemoteTerminalConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTerminalConfig {
    pub id: String,
    pub remote_workspace_id: String,
    pub name: String,
    pub sort_order: i64,
}

fn remote_db_error(error: sqlx::Error) -> String {
    if let sqlx::Error::Database(detail) = &error {
        let message = detail.message();
        if detail.is_unique_violation() {
            if message.contains("t_ssh_known_host") || message.contains("host, port, algorithm") {
                return "该端点的这个算法已经有主机密钥记录".into();
            }
            return "远程连接记录冲突".into();
        }
        if detail.is_foreign_key_violation() {
            return "远程连接或工作区不存在".into();
        }
    }
    db_error(error)
}

impl Database {
    pub async fn list_ssh_connections(&self) -> Result<Vec<SshConnection>> {
        let rows = sqlx::query("SELECT id, name, host, port, username, auth_type, private_key_path, sort_order FROM t_ssh_connection ORDER BY sort_order, id")
            .fetch_all(&self.pool).await.map_err(remote_db_error)?;
        rows.into_iter().map(connection_from_row).collect()
    }

    pub async fn create_ssh_connection(&self, input: SshConnectionInput) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO t_ssh_connection(id, name, host, port, username, auth_type, private_key_path, sort_order) VALUES (?, ?, ?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM t_ssh_connection))")
            .bind(&id)
            .bind(&input.name)
            .bind(&input.host)
            .bind(i64::from(input.port))
            .bind(&input.username)
            .bind(input.auth_type.as_str())
            .bind(&input.private_key_path)
            .execute(&self.pool)
            .await
            .map_err(remote_db_error)?;
        Ok(id)
    }

    pub async fn update_ssh_connection(&self, id: &str, input: SshConnectionInput) -> Result<()> {
        let result = sqlx::query("UPDATE t_ssh_connection SET name = ?, host = ?, port = ?, username = ?, auth_type = ?, private_key_path = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(&input.name)
            .bind(&input.host)
            .bind(i64::from(input.port))
            .bind(&input.username)
            .bind(input.auth_type.as_str())
            .bind(&input.private_key_path)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(remote_db_error)?;
        require_changed(result.rows_affected())
    }

    pub async fn delete_ssh_connection(&self, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM t_ssh_connection WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(remote_db_error)?;
        require_changed(result.rows_affected())
    }

    pub async fn list_known_host_views(&self, host: &str, port: u16) -> Result<Vec<KnownHost>> {
        let (host, port) = normalize_host(host, port)?;
        let rows = sqlx::query("SELECT id, host, port, algorithm, fingerprint_sha256 FROM t_ssh_known_host WHERE host = ? AND port = ? ORDER BY algorithm")
            .bind(host)
            .bind(i64::from(port))
            .fetch_all(&self.pool)
            .await
            .map_err(remote_db_error)?;
        Ok(rows.into_iter().map(|row| KnownHost {
            id: row.get("id"),
            host: row.get("host"),
            port: u16::try_from(row.get::<i64, _>("port")).unwrap_or(22),
            algorithm: row.get("algorithm"),
            fingerprint_sha256: row.get("fingerprint_sha256"),
        }).collect())
    }

    pub async fn list_known_host_records(&self, host: &str, port: u16) -> Result<Vec<KnownHostRecord>> {
        let (host, port) = normalize_host(host, port)?;
        let rows = sqlx::query("SELECT algorithm, public_key, fingerprint_sha256 FROM t_ssh_known_host WHERE host = ? AND port = ?")
            .bind(host)
            .bind(i64::from(port))
            .fetch_all(&self.pool)
            .await
            .map_err(remote_db_error)?;
        Ok(rows.into_iter().map(|row| KnownHostRecord {
            algorithm: row.get("algorithm"),
            public_key: row.get("public_key"),
            fingerprint_sha256: row.get("fingerprint_sha256"),
        }).collect())
    }

    pub async fn save_known_host(&self, host: &str, port: u16, algorithm: &str, public_key: &[u8], fingerprint: &str) -> Result<()> {
        let (host, port) = normalize_host(host, port)?;
        sqlx::query("INSERT INTO t_ssh_known_host(id, host, port, algorithm, public_key, fingerprint_sha256) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(Uuid::new_v4().to_string())
            .bind(host)
            .bind(i64::from(port))
            .bind(algorithm)
            .bind(public_key)
            .bind(fingerprint)
            .execute(&self.pool)
            .await
            .map_err(remote_db_error)?;
        Ok(())
    }

    pub async fn forget_known_host(&self, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM t_ssh_known_host WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(remote_db_error)?;
        require_changed(result.rows_affected())
    }

    pub async fn open_remote_workspace(&self, connection_id: &str) -> Result<RemoteWorkspaceRecord> {
        let mut tx = self.pool.begin().await.map_err(remote_db_error)?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM t_ssh_connection WHERE id = ?)")
            .bind(connection_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(remote_db_error)?;
        if !exists {
            return Err("远程连接不存在".into());
        }
        let workspace_id: Option<String> = sqlx::query_scalar("SELECT id FROM t_remote_workspace WHERE connection_id = ?")
            .bind(connection_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(remote_db_error)?;
        let workspace_id = if let Some(id) = workspace_id {
            id
        } else {
            let id = Uuid::new_v4().to_string();
            let layout = empty_remote_layout().to_string();
            sqlx::query("INSERT INTO t_remote_workspace(id, connection_id, schema_version, layout_json) VALUES (?, ?, 1, ?)")
                .bind(&id)
                .bind(connection_id)
                .bind(&layout)
                .execute(&mut *tx)
                .await
                .map_err(remote_db_error)?;
            let terminal_id = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO t_remote_terminal_config(id, remote_workspace_id, name, sort_order) VALUES (?, ?, ?, 0)")
                .bind(&terminal_id)
                .bind(&id)
                .bind("Terminal")
                .execute(&mut *tx)
                .await
                .map_err(remote_db_error)?;
            let root = serde_json::json!({"type":"terminal","terminalConfigId":terminal_id});
            let layout = serde_json::json!({"schemaVersion":1,"activeTabId":"main","tabs":[{"id":"main","title":"Terminal","root":root}]}).to_string();
            sqlx::query("UPDATE t_remote_workspace SET layout_json = ? WHERE id = ?")
                .bind(layout)
                .bind(&id)
                .execute(&mut *tx)
                .await
                .map_err(remote_db_error)?;
            id
        };
        tx.commit().await.map_err(remote_db_error)?;
        self.load_remote_workspace(&workspace_id).await
    }

    pub async fn load_remote_workspace(&self, workspace_id: &str) -> Result<RemoteWorkspaceRecord> {
        let row = sqlx::query("SELECT id, connection_id, layout_json, last_directory FROM t_remote_workspace WHERE id = ?")
            .bind(workspace_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(remote_db_error)?
            .ok_or_else(|| "远程工作区不存在".to_owned())?;
        let terminals = self.list_remote_terminals(workspace_id).await?;
        let configs = terminals.iter().map(|terminal| terminal.id.clone()).collect::<HashSet<_>>();
        let raw: String = row.get("layout_json");
        let layout = RemoteLayout::repair(&raw, &configs);
        if serde_json::to_string(&layout).ok().as_deref() != Some(raw.as_str()) {
            let _ = self.save_remote_layout(workspace_id, &layout).await;
        }
        Ok(RemoteWorkspaceRecord {
            id: row.get("id"),
            connection_id: row.get("connection_id"),
            layout,
            last_directory: row.get("last_directory"),
            terminals,
        })
    }

    pub async fn save_remote_layout(&self, workspace_id: &str, layout: &RemoteLayout) -> Result<()> {
        let terminals = self.list_remote_terminals(workspace_id).await?;
        let configs = terminals.iter().map(|terminal| terminal.id.clone()).collect();
        layout.validate(&configs)?;
        let result = sqlx::query("UPDATE t_remote_workspace SET schema_version = ?, layout_json = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(i64::from(layout.schema_version))
            .bind(serde_json::to_string(layout).map_err(|error| error.to_string())?)
            .bind(workspace_id)
            .execute(&self.pool)
            .await
            .map_err(remote_db_error)?;
        require_changed(result.rows_affected())
    }

    pub async fn set_remote_directory(&self, workspace_id: &str, directory: Option<&str>) -> Result<()> {
        let result = sqlx::query("UPDATE t_remote_workspace SET last_directory = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(directory)
            .bind(workspace_id)
            .execute(&self.pool)
            .await
            .map_err(remote_db_error)?;
        require_changed(result.rows_affected())
    }

    pub async fn list_remote_terminals(&self, workspace_id: &str) -> Result<Vec<RemoteTerminalConfig>> {
        let rows = sqlx::query("SELECT id, remote_workspace_id, name, sort_order FROM t_remote_terminal_config WHERE remote_workspace_id = ? ORDER BY sort_order, id")
            .bind(workspace_id)
            .fetch_all(&self.pool)
            .await
            .map_err(remote_db_error)?;
        Ok(rows.into_iter().map(|row| RemoteTerminalConfig {
            id: row.get("id"),
            remote_workspace_id: row.get("remote_workspace_id"),
            name: row.get("name"),
            sort_order: row.get("sort_order"),
        }).collect())
    }

    pub async fn create_remote_terminal(&self, workspace_id: &str, name: &str) -> Result<String> {
        let name = orbyte_core::validate_name(name)?;
        let id = Uuid::new_v4().to_string();
        let result = sqlx::query("INSERT INTO t_remote_terminal_config(id, remote_workspace_id, name, sort_order) SELECT ?, id, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM t_remote_terminal_config WHERE remote_workspace_id = t_remote_workspace.id) FROM t_remote_workspace WHERE id = ?")
            .bind(&id)
            .bind(name)
            .bind(workspace_id)
            .execute(&self.pool)
            .await
            .map_err(remote_db_error)?;
        if result.rows_affected() == 0 {
            return Err("远程工作区不存在".into());
        }
        Ok(id)
    }

    pub async fn delete_remote_terminal(&self, workspace_id: &str, terminal_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(remote_db_error)?;
        let result = sqlx::query("DELETE FROM t_remote_terminal_config WHERE id = ? AND remote_workspace_id = ?")
            .bind(terminal_id)
            .bind(workspace_id)
            .execute(&mut *tx)
            .await
            .map_err(remote_db_error)?;
        require_changed(result.rows_affected())?;
        let raw: String = sqlx::query_scalar("SELECT layout_json FROM t_remote_workspace WHERE id = ?")
            .bind(workspace_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(remote_db_error)?;
        let terminals = sqlx::query_scalar::<_, String>("SELECT id FROM t_remote_terminal_config WHERE remote_workspace_id = ?")
            .bind(workspace_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(remote_db_error)?;
        let layout = RemoteLayout::repair(&raw, &terminals.into_iter().collect());
        sqlx::query("UPDATE t_remote_workspace SET layout_json = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(serde_json::to_string(&layout).map_err(|error| error.to_string())?)
            .bind(workspace_id)
            .execute(&mut *tx)
            .await
            .map_err(remote_db_error)?;
        tx.commit().await.map_err(remote_db_error)
    }

    pub async fn ssh_connection(&self, id: &str) -> Result<SshConnection> {
        let row = sqlx::query("SELECT id, name, host, port, username, auth_type, private_key_path, sort_order FROM t_ssh_connection WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(remote_db_error)?
            .ok_or_else(|| "远程连接不存在".to_owned())?;
        connection_from_row(row)
    }
}

fn normalize_host(host: &str, port: u16) -> Result<(String, u16)> {
    orbyte_core::normalize_endpoint(host, Some(port))
}

fn connection_from_row(row: sqlx::sqlite::SqliteRow) -> Result<SshConnection> {
    let port = u16::try_from(row.get::<i64, _>("port")).map_err(|_| "端口无效".to_owned())?;
    Ok(SshConnection {
        id: row.get("id"),
        name: row.get("name"),
        host: row.get("host"),
        port,
        username: row.get("username"),
        auth_type: row.get("auth_type"),
        private_key_path: row.get("private_key_path"),
        sort_order: row.get("sort_order"),
    })
}

pub fn parse_connection(name: &str, host: &str, port: Option<u16>, username: &str, auth_type: &str, private_key_path: Option<&str>) -> Result<SshConnectionInput> {
    validate_ssh_connection(name, host, port, username, auth_type, private_key_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    async fn memory() -> Database {
        Database::open(Path::new(":memory:")).await.unwrap()
    }

    #[tokio::test]
    async fn connections_keep_separate_users_and_never_store_a_password_column() {
        let db = memory().await;
        let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('t_ssh_connection')")
            .fetch_all(&db.pool).await.unwrap();
        assert!(!columns.iter().any(|column| column.contains("password") || column.contains("passphrase") || column == "secret"));
        let first = db.create_ssh_connection(parse_connection("lab", "Example.COM", None, "root", "password", None).unwrap()).await.unwrap();
        let second = db.create_ssh_connection(parse_connection("lab", "example.com", Some(22), "dev", "privateKey", Some("C:/keys/id_ed25519")).unwrap()).await.unwrap();
        let listed = db.list_ssh_connections().await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].host, "example.com");
        assert_eq!(db.ssh_connection(&first).await.unwrap().port, 22);
        db.save_known_host("example.com", 22, "ssh-ed25519", b"ed", "SHA256:ed").await.unwrap();
        db.save_known_host("Example.COM", 22, "rsa-sha2-256", b"rsa", "SHA256:rsa").await.unwrap();
        let keys = db.list_known_host_records("example.com", 22).await.unwrap();
        assert_eq!(keys.len(), 2);
        db.delete_ssh_connection(&second).await.unwrap();
        assert_eq!(db.list_ssh_connections().await.unwrap().len(), 1);
        let _ = first;
    }

    #[tokio::test]
    async fn equivalent_ipv6_hits_the_same_known_host_and_a_bad_layout_keeps_the_connection() {
        let db = memory().await;
        let (host, port) = orbyte_core::normalize_endpoint("0:0:0:0:0:0:0:1", Some(22)).unwrap();
        let id = db.create_ssh_connection(parse_connection("loop", &host, Some(port), "root", "password", None).unwrap()).await.unwrap();
        db.save_known_host(&host, port, "ssh-ed25519", b"key", "SHA256:key").await.unwrap();
        let (again, _) = orbyte_core::normalize_endpoint("[::1]", Some(22)).unwrap();
        assert_eq!(db.list_known_host_records(&again, 22).await.unwrap().len(), 1);
        let workspace = db.open_remote_workspace(&id).await.unwrap();
        sqlx::query("UPDATE t_remote_workspace SET layout_json = ? WHERE id = ?")
            .bind("{not-json")
            .bind(&workspace.id)
            .execute(&db.pool)
            .await
            .unwrap();
        let loaded = db.load_remote_workspace(&workspace.id).await.unwrap();
        assert!(loaded.layout.tabs.is_empty() || loaded.layout.validate(&loaded.terminals.iter().map(|t| t.id.clone()).collect()).is_ok());
        assert!(db.ssh_connection(&id).await.is_ok());
        let local_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM t_terminal_config").fetch_one(&db.pool).await.unwrap();
        assert_eq!(local_before, 0);
    }

    #[tokio::test]
    async fn deleting_a_connection_cascades_only_its_remote_rows() {
        let db = memory().await;
        let id = db.create_ssh_connection(parse_connection("one", "one.example", Some(22), "a", "password", None).unwrap()).await.unwrap();
        let other = db.create_ssh_connection(parse_connection("two", "two.example", Some(22), "b", "password", None).unwrap()).await.unwrap();
        let workspace = db.open_remote_workspace(&id).await.unwrap();
        let kept = db.open_remote_workspace(&other).await.unwrap();
        db.delete_ssh_connection(&id).await.unwrap();
        assert!(db.load_remote_workspace(&workspace.id).await.is_err());
        assert!(db.load_remote_workspace(&kept.id).await.is_ok());
    }
}
