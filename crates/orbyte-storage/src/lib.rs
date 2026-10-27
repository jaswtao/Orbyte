//! SQLite 持久化。所有多表修改在事务中完成。
//! Author: jaswtao
//! Date: 2026-10-12

use orbyte_core::{
    default_settings, validate_name, validate_setting, Project, ProjectGroup, ProjectInput,
};
use serde_json::Value;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};
use uuid::Uuid;

type Result<T> = std::result::Result<T, String>;

mod draft;
pub use draft::{EditorDraft, MAX_DRAFT_BYTES};
mod terminal;
mod layout;
mod migration;

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

#[track_caller]
fn db_error(error: sqlx::Error) -> String {
    let location = std::panic::Location::caller();
    if let sqlx::Error::Database(ref detail) = error {
        log::error!("database_operation_failed code={:?} location={location}", detail.code());
    } else {
        log::error!("database_operation_failed error={error} location={location}");
    }
    if let sqlx::Error::Database(ref error) = error {
        if error.is_unique_violation() {
            return "该项目目录已经添加，请勿重复添加".into();
        }
        if error.is_foreign_key_violation() {
            return "分组不存在或仍包含项目，请先移动或删除组内项目".into();
        }
    }
    format!("数据库操作失败：{error}")
}

fn require_changed(count: u64) -> Result<()> {
    if count == 0 {
        Err("记录已不存在，请刷新后重试".into())
    } else {
        Ok(())
    }
}

impl Database {
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        // 单写连接也让读-改-写事务按提交顺序执行，避免 SQLite 升级锁竞争。
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(db_error)?;
        if let Err(error) = migration::run(&pool).await {
            pool.close().await;
            return Err(format!("数据库迁移失败，已停止数据写入：{error}"));
        }
        Ok(Self { pool })
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        let rows = sqlx::query("SELECT p.*, w.id AS workspace_id FROM t_project p JOIN t_workspace w ON w.project_id = p.id ORDER BY p.sort_order, p.id")
            .fetch_all(&self.pool).await.map_err(db_error)?;
        Ok(rows
            .into_iter()
            .map(|r| Project {
                id: r.get("id"),
                group_id: r.get("group_id"),
                name: r.get("name"),
                path: r.get("path"),
                favorite: r.get("favorite"),
                hidden: r.get("hidden"),
                sort_order: r.get("sort_order"),
                workspace_id: r.get("workspace_id"),
            })
            .collect())
    }

    /// path 在服务边界规范化后传入；数据库以 NOCASE 再次约束唯一性。
    pub async fn create_project(&self, input: ProjectInput) -> Result<String> {
        let name = validate_name(&input.name)?;
        let id = Uuid::new_v4().to_string();
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        sqlx::query("INSERT INTO t_project(id, group_id, name, path, favorite, sort_order) VALUES (?, ?, ?, ?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM t_project))")
            .bind(&id).bind(input.group_id).bind(name).bind(input.path).bind(input.favorite)
            .execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("INSERT INTO t_workspace(id, project_id) VALUES (?, ?)")
            .bind(Uuid::new_v4().to_string())
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(id)
    }

    pub async fn set_project_hidden(&self, id: &str, hidden: bool, replacement_id: Option<&str>) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let result = sqlx::query("UPDATE t_project SET hidden = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(hidden).bind(id).execute(&mut *tx).await.map_err(db_error)?;
        require_changed(result.rows_affected())?;
        let active: Option<String> = sqlx::query_scalar("SELECT value_json FROM t_app_setting WHERE key = 'active_project_id'")
            .fetch_optional(&mut *tx).await.map_err(db_error)?;
        if hidden && active.as_deref().and_then(|value| serde_json::from_str::<Value>(value).ok()) == Some(Value::String(id.into())) {
            if let Some(replacement) = replacement_id {
                let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM t_project WHERE id = ? AND id != ? AND hidden = 0)")
                    .bind(replacement).bind(id).fetch_one(&mut *tx).await.map_err(db_error)?;
                if !valid { return Err("替代项目不存在或已隐藏".into()); }
            }
            sqlx::query("INSERT INTO t_app_setting(key, value_json) VALUES ('active_project_id', ?) ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json")
                .bind(serde_json::json!(replacement_id).to_string()).execute(&mut *tx).await.map_err(db_error)?;
        }
        tx.commit().await.map_err(db_error)
    }

    pub async fn update_project(&self, id: &str, input: ProjectInput) -> Result<()> {
        let name = validate_name(&input.name)?;
        let result = sqlx::query("UPDATE t_project SET group_id = ?, name = ?, path = ?, favorite = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(input.group_id).bind(name).bind(input.path).bind(input.favorite).bind(id)
            .execute(&self.pool).await.map_err(db_error)?;
        require_changed(result.rows_affected())
    }

    pub async fn delete_project(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        sqlx::query("DELETE FROM t_app_setting WHERE key = 'active_project_id' AND json_extract(value_json, '$') = ?")
            .bind(id).execute(&mut *tx).await.map_err(db_error)?;
        let result = sqlx::query("DELETE FROM t_project WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        require_changed(result.rows_affected())?;
        tx.commit().await.map_err(db_error)
    }

    pub async fn list_groups(&self) -> Result<Vec<ProjectGroup>> {
        let rows =
            sqlx::query("SELECT id, name, sort_order FROM t_project_group ORDER BY sort_order, id")
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?;
        Ok(rows
            .into_iter()
            .map(|r| ProjectGroup {
                id: r.get("id"),
                name: r.get("name"),
                sort_order: r.get("sort_order"),
            })
            .collect())
    }

    pub async fn create_group(&self, name: &str) -> Result<String> {
        let name = validate_name(name)?;
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO t_project_group(id, name, sort_order) VALUES (?, ?, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM t_project_group))")
            .bind(&id).bind(name).execute(&self.pool).await.map_err(db_error)?;
        Ok(id)
    }

    pub async fn update_group(&self, id: &str, name: &str) -> Result<()> {
        let result = sqlx::query(
            "UPDATE t_project_group SET name = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(validate_name(name)?)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        require_changed(result.rows_affected())
    }

    pub async fn delete_group(&self, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM t_project_group WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        require_changed(result.rows_affected())
    }

    /// 接收完整项目顺序和分组，单条 UPDATE 实现组内及跨组移动。
    pub async fn reorder_projects(&self, entries: Vec<(String, Option<String>)>) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM t_project")
            .fetch_all(&mut *tx)
            .await
            .map_err(db_error)?;
        validate_order(&ids, entries.iter().map(|v| v.0.as_str()))?;
        let json = serde_json::to_string(&entries).map_err(|e| e.to_string())?;
        sqlx::query("UPDATE t_project SET sort_order = (SELECT CAST(key AS INTEGER) FROM json_each(?) WHERE json_extract(value, '$[0]') = t_project.id), group_id = (SELECT json_extract(value, '$[1]') FROM json_each(?) WHERE json_extract(value, '$[0]') = t_project.id), updated_at = CURRENT_TIMESTAMP")
            .bind(&json).bind(&json).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)
    }

    pub async fn reorder_groups(&self, ids: Vec<String>) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let current: Vec<String> = sqlx::query_scalar("SELECT id FROM t_project_group")
            .fetch_all(&mut *tx)
            .await
            .map_err(db_error)?;
        validate_order(&current, ids.iter().map(String::as_str))?;
        sqlx::query("UPDATE t_project_group SET sort_order = (SELECT CAST(key AS INTEGER) FROM json_each(?) WHERE value = t_project_group.id), updated_at = CURRENT_TIMESTAMP")
            .bind(serde_json::to_string(&ids).map_err(|e| e.to_string())?).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)
    }

    pub async fn get_settings(&self) -> Result<BTreeMap<String, Value>> {
        let mut settings = default_settings();
        let mut explicitly_empty = false;
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let rows = sqlx::query("SELECT key, value_json FROM t_app_setting")
            .fetch_all(&mut *tx)
            .await
            .map_err(db_error)?;
        for row in rows {
            let key: String = row.get("key");
            let raw: String = row.get("value_json");
            let value: Value =
                serde_json::from_str(&raw).map_err(|e| format!("设置 {key} 无法读取：{e}"))?;
            validate_setting(&key, &value)?;
            if key == "active_project_id" && value.is_null() { explicitly_empty = true; }
            settings.insert(key, value);
        }
        let active = settings["active_project_id"].as_str();
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM t_project WHERE id = ?)")
                .bind(active)
                .fetch_one(&mut *tx)
                .await
                .map_err(db_error)?;
        if !exists && !explicitly_empty {
            let first: Option<String> =
                sqlx::query_scalar("SELECT id FROM t_project WHERE hidden = 0 OR ? ORDER BY sort_order, id LIMIT 1")
                    .bind(settings["show_hidden_projects"].as_bool().unwrap_or(false))
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(db_error)?;
            let value = first.map(Value::String).unwrap_or(Value::Null);
            sqlx::query("INSERT INTO t_app_setting(key, value_json) VALUES ('active_project_id', ?) ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json")
                .bind(value.to_string()).execute(&mut *tx).await.map_err(db_error)?;
            settings.insert("active_project_id".into(), value);
        }
        tx.commit().await.map_err(db_error)?;
        Ok(settings)
    }

    pub async fn set_setting(&self, key: &str, value: Value) -> Result<()> {
        self.set_settings(BTreeMap::from([(key.to_owned(), value)])).await
    }

    /// 已知设置一次批量写入，任一参数非法时不修改任何设置。
    pub async fn set_settings(&self, values: BTreeMap<String, Value>) -> Result<()> {
        for (key, value) in &values {
            validate_setting(key, value)?;
        }
        if values.is_empty() { return Ok(()); }
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        if let Some(value) = values.get("active_project_id").filter(|value| !value.is_null()) {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM t_project WHERE id = ?)")
                    .bind(value.as_str())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(db_error)?;
            if !exists {
                return Err("要激活的项目不存在".into());
            }
        }
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("INSERT INTO t_app_setting(key, value_json) ");
        query.push_values(&values, |mut row, (key, value)| {
            row.push_bind(key).push_bind(value.to_string());
        });
        query.push(" ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json");
        query.build().execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)
    }
}

fn validate_order<'a>(current: &[String], ids: impl Iterator<Item = &'a str>) -> Result<()> {
    let ids: Vec<_> = ids.collect();
    let unique: HashSet<_> = ids.iter().copied().collect();
    if ids.len() != current.len()
        || unique.len() != ids.len()
        || !current.iter().all(|id| unique.contains(id.as_str()))
    {
        return Err("排序列表与当前记录不一致，请刷新后重试".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
