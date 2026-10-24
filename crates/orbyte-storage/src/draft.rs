//! Local recovery copies, independent of source files. Author: jaswtao · Date: 2026-10-24
use super::{Database, Result};
use serde::{Deserialize, Serialize};
use sqlx::Row;

pub const MAX_DRAFT_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorDraft {
    pub project_id: String,
    pub path: String,
    pub project_path: String,
    pub content: String,
    pub baseline: String,
    pub encoding: String,
    pub bom: bool,
    pub line_ending: String,
    pub revision: String,
    #[serde(default)]
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbyte_core::ProjectInput;
    use std::path::Path;

    async fn fixture() -> (Database, EditorDraft) {
        let db = Database::open(Path::new(":memory:")).await.unwrap();
        let project = db.create_project(ProjectInput { name: "draft test".into(), path: "C:/draft-test".into(), group_id: None, favorite: false }).await.unwrap();
        (db, EditorDraft { project_id: project, path: "dir/file.txt".into(), project_path: "C:/draft-test".into(), content: "unsaved".into(), baseline: "disk-version".into(), encoding: "utf-8".into(), bom: false, line_ending: "LF".into(), revision: "one".into(), updated_at: String::new() })
    }

    #[tokio::test]
    async fn upsert_case_insensitive_cleanup_is_revision_conditional_and_cascades() {
        let (db, mut draft) = fixture().await;
        db.put_editor_draft(&draft).await.unwrap();
        draft.path = "DIR/FILE.txt".into(); draft.content = "newest".into(); draft.revision = "two".into();
        db.put_editor_draft(&draft).await.unwrap();
        db.delete_editor_draft(&draft.project_id, "dir/file.txt", "one").await.unwrap();
        let all = db.list_editor_drafts().await.unwrap();
        assert_eq!(all.len(), 1); assert_eq!(all[0].content, "newest");
        db.delete_editor_draft(&draft.project_id, "dir/file.txt", "two").await.unwrap();
        assert!(db.list_editor_drafts().await.unwrap().is_empty());
        db.put_editor_draft(&draft).await.unwrap();
        db.delete_project(&draft.project_id).await.unwrap();
        assert!(db.list_editor_drafts().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn prefix_rename_is_literal_atomic_and_project_isolated() {
        let (db, mut draft) = fixture().await;
        draft.path = "dir_100%/file.txt".into();
        db.put_editor_draft(&draft).await.unwrap();
        let other = db.create_project(ProjectInput { name: "other".into(), path: "C:/other".into(), group_id: None, favorite: false }).await.unwrap();
        let mut second = draft.clone(); second.project_id = other; second.project_path = "C:/other".into();
        db.put_editor_draft(&second).await.unwrap();
        db.rename_editor_drafts(&draft.project_id, &draft.project_path, "dir_100%", "new").await.unwrap();
        let all = db.list_editor_drafts().await.unwrap();
        assert!(all.iter().any(|d| d.project_id == draft.project_id && d.path == "new/file.txt"));
        assert!(all.iter().any(|d| d.project_id == second.project_id && d.path == second.path));
        draft.path = "occupied/file.txt".into(); db.put_editor_draft(&draft).await.unwrap();
        assert!(db.rename_editor_drafts(&draft.project_id, &draft.project_path, "new", "occupied").await.is_err());
        assert!(db.list_editor_drafts().await.unwrap().iter().any(|d| d.path == "new/file.txt"));
    }

    #[tokio::test]
    async fn size_path_and_project_root_validation_preserve_existing_copy() {
        let (db, mut draft) = fixture().await;
        db.put_editor_draft(&draft).await.unwrap();
        draft.project_path = "C:/changed".into(); assert!(db.put_editor_draft(&draft).await.is_err());
        draft.project_path = "C:/draft-test".into(); draft.path = "../escape".into(); assert!(db.put_editor_draft(&draft).await.is_err());
        draft.path = "dir/file.txt".into(); draft.content = "x".repeat(MAX_DRAFT_BYTES+1); assert!(db.put_editor_draft(&draft).await.is_err());
        draft.content.truncate(MAX_DRAFT_BYTES);
        let started = std::time::Instant::now(); db.put_editor_draft(&draft).await.unwrap();
        eprintln!("draft_5MiB_write_ms={:.3}", started.elapsed().as_secs_f64()*1000.0);
        let started = std::time::Instant::now(); db.set_setting("font_size", serde_json::json!(14)).await.unwrap();
        eprintln!("setting_after_draft_ms={:.3}", started.elapsed().as_secs_f64()*1000.0);
        assert_eq!(db.list_editor_drafts().await.unwrap()[0].content.len(), MAX_DRAFT_BYTES);
    }

    #[tokio::test]
    async fn durable_draft_survives_reopen_and_shares_the_write_connection() {
        let directory = std::env::temp_dir().join(format!("orbyte-draft-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("draft.db");
        let db = Database::open(&path).await.unwrap();
        let project = db.create_project(ProjectInput { name: "durable".into(), path: "C:/durable".into(), group_id: None, favorite: false }).await.unwrap();
        let draft = EditorDraft { project_id: project, path: "missing.txt".into(), project_path: "C:/durable".into(),
            content: "x".repeat(MAX_DRAFT_BYTES), baseline: "v1".into(), encoding: "utf-8".into(), bom: false,
            line_ending: "LF".into(), revision: "durable-revision".into(), updated_at: String::new() };
        let write = async {
            let start = std::time::Instant::now();
            db.put_editor_draft(&draft).await.unwrap();
            eprintln!("durable_5MiB_write_ms={:.3}", start.elapsed().as_secs_f64()*1000.0);
        };
        let setting = async {
            let start = std::time::Instant::now();
            db.set_setting("font_size", serde_json::json!(14)).await.unwrap();
            eprintln!("setting_queued_with_draft_ms={:.3}", start.elapsed().as_secs_f64()*1000.0);
        };
        tokio::join!(write, setting);
        db.pool.close().await;
        let reopened = Database::open(&path).await.unwrap();
        let restored = reopened.list_editor_drafts().await.unwrap();
        assert_eq!(restored[0].content.len(), MAX_DRAFT_BYTES);
        assert_eq!(restored[0].revision, "durable-revision");
        reopened.pool.close().await;
        std::fs::remove_dir_all(directory).unwrap();
    }
}

// Never log SQL errors or values here: they may include draft text.
fn draft_error(_: sqlx::Error) -> String { "Draft storage operation failed; retry without discarding your changes".into() }

impl Database {
    pub async fn list_editor_drafts(&self) -> Result<Vec<EditorDraft>> {
        let rows = sqlx::query("SELECT * FROM t_editor_draft ORDER BY updated_at DESC")
            .fetch_all(&self.pool).await.map_err(draft_error)?;
        Ok(rows.into_iter().map(|r| EditorDraft {
            project_id: r.get("project_id"), path: r.get("path"), project_path: r.get("project_path"),
            content: r.get("content"), baseline: r.get("baseline"), encoding: r.get("encoding"),
            bom: r.get("bom"), line_ending: r.get("line_ending"), revision: r.get("revision"), updated_at: r.get("updated_at"),
        }).collect())
    }

    pub async fn put_editor_draft(&self, draft: &EditorDraft) -> Result<()> {
        orbyte_core::validate_relative_path(&draft.path)?;
        if draft.path.is_empty() || draft.content.len() > MAX_DRAFT_BYTES || draft.revision.is_empty()
            || !["utf-8", "gbk"].contains(&draft.encoding.as_str()) || !["LF", "CRLF"].contains(&draft.line_ending.as_str()) {
            return Err("Invalid draft metadata or draft exceeds 5 MiB".into());
        }
        // Capture the root on initial insertion. A changed project root must never claim an old draft.
        let result = sqlx::query("INSERT INTO t_editor_draft(project_id,path,project_path,content,baseline,encoding,bom,line_ending,revision) SELECT ?,?,path,?,?,?,?,?,? FROM t_project WHERE id = ? AND path = ? COLLATE NOCASE ON CONFLICT(project_id,path) DO UPDATE SET content=excluded.content,baseline=excluded.baseline,encoding=excluded.encoding,bom=excluded.bom,line_ending=excluded.line_ending,revision=excluded.revision,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE t_editor_draft.project_path = excluded.project_path COLLATE NOCASE")
            .bind(&draft.project_id).bind(&draft.path).bind(&draft.content).bind(&draft.baseline).bind(&draft.encoding)
            .bind(draft.bom).bind(&draft.line_ending).bind(&draft.revision).bind(&draft.project_id).bind(&draft.project_path)
            .execute(&self.pool).await.map_err(draft_error)?;
        if result.rows_affected() == 0 { return Err("Project path changed; review the existing draft first".into()); }
        Ok(())
    }

    pub async fn delete_editor_draft(&self, project: &str, path: &str, revision: &str) -> Result<()> {
        sqlx::query("DELETE FROM t_editor_draft WHERE project_id=? AND path=? AND revision=?")
            .bind(project).bind(path).bind(revision).execute(&self.pool).await.map_err(draft_error)?;
        Ok(())
    }

    pub async fn rename_editor_drafts(&self, project: &str, root: &str, from: &str, to: &str) -> Result<()> {
        orbyte_core::validate_relative_path(from)?;
        orbyte_core::validate_relative_path(to)?;
        if from.is_empty() || to.is_empty() { return Err("Draft path cannot be empty".into()); }
        let mut tx = self.pool.begin().await.map_err(draft_error)?;
        // substr avoids LIKE wildcard interpretation for valid names containing '%' or '_'.
        sqlx::query("UPDATE t_editor_draft SET path=? || substr(path,length(?)+1) WHERE project_id=? AND project_path=? COLLATE NOCASE AND (path=? COLLATE NOCASE OR substr(path,1,length(?)+1)=(? || '/') COLLATE NOCASE)")
            .bind(to).bind(from).bind(project).bind(root).bind(from).bind(from).bind(from)
            .execute(&mut *tx).await.map_err(draft_error)?;
        tx.commit().await.map_err(draft_error)
    }
}
