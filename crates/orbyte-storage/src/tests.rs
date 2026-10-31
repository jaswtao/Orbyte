//! 持久化、事务与约束回归测试。
//! Author: jaswtao
//! Date: 2026-10-12

use super::*;
use serde_json::json;

fn input(path: &str, group_id: Option<&str>) -> ProjectInput {
    ProjectInput {
        name: "财务项目".into(),
        path: path.into(),
        group_id: group_id.map(str::to_owned),
        favorite: true,
    }
}

async fn memory() -> Database {
    Database::open(Path::new(":memory:")).await.unwrap()
}

#[tokio::test]
async fn hiding_projects_switches_atomically_and_preserves_other_fields() {
    let db = memory().await;
    let a = db.create_project(input("C:/hidden-a", None)).await.unwrap();
    let b = db.create_project(input("C:/hidden-b", None)).await.unwrap();
    assert!(db.list_projects().await.unwrap().iter().all(|p| !p.hidden));
    assert_eq!(db.get_settings().await.unwrap()["show_hidden_projects"], json!(false));
    db.set_setting("active_project_id", json!(a)).await.unwrap();
    for replacement in ["missing", a.as_str()] {
        assert!(db.set_project_hidden(&a, true, Some(replacement)).await.is_err());
        assert!(!db.list_projects().await.unwrap()[0].hidden);
        assert_eq!(db.get_settings().await.unwrap()["active_project_id"], json!(a));
    }
    db.set_project_hidden(&b, true, None).await.unwrap();
    assert!(db.set_project_hidden(&a, true, Some(&b)).await.is_err());
    assert!(!db.list_projects().await.unwrap()[0].hidden);
    db.set_project_hidden(&b, false, None).await.unwrap();
    db.set_project_hidden(&a, true, Some(&b)).await.unwrap();
    assert_eq!(db.get_settings().await.unwrap()["active_project_id"], json!(b));
    let before = db.list_projects().await.unwrap()[0].clone();
    let mut edit = input("C:/hidden-a", None);
    edit.name = "Edited".into();
    edit.favorite = false;
    db.update_project(&a, edit).await.unwrap();
    db.reorder_projects(vec![(b.clone(), None), (a.clone(), None)]).await.unwrap();
    let after = db.list_projects().await.unwrap().into_iter().find(|p| p.id == a).unwrap();
    assert!(after.hidden);
    assert_eq!(after.workspace_id, before.workspace_id);
    assert_eq!(after.path, before.path);
    assert!(!after.favorite);
    db.set_project_hidden(&a, false, None).await.unwrap();
    assert_eq!(db.get_settings().await.unwrap()["active_project_id"], json!(b));
    assert!(db.set_project_hidden("missing", true, None).await.is_err());
}

#[tokio::test]
async fn empty_workspace_and_visibility_survive_reopening() {
    let directory = std::env::temp_dir().join(format!("orbyte-hidden-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("hidden.db");
    let db = Database::open(&path).await.unwrap();
    let a = db.create_project(input("C:/only-hidden", None)).await.unwrap();
    let b = db.create_project(input("C:/filtered-out", None)).await.unwrap();
    db.set_setting("active_project_id", json!(a)).await.unwrap();
    // No replacement can also mean other projects do not match the sidebar filters.
    db.set_project_hidden(&a, true, None).await.unwrap();
    assert!(db.get_settings().await.unwrap()["active_project_id"].is_null());
    db.set_setting("show_hidden_projects", json!(true)).await.unwrap();
    db.pool.close().await;
    let db = Database::open(&path).await.unwrap();
    let settings = db.get_settings().await.unwrap();
    assert!(settings["active_project_id"].is_null());
    assert_eq!(settings["show_hidden_projects"], json!(true));
    assert!(db.list_projects().await.unwrap().iter().find(|p| p.id == a).unwrap().hidden);
    db.set_setting("active_project_id", json!(a)).await.unwrap();
    assert!(db.set_settings(BTreeMap::from([
        ("show_hidden_projects".into(), json!(false)),
        ("active_project_id".into(), json!("missing")),
    ])).await.is_err());
    assert_eq!(db.get_settings().await.unwrap()["show_hidden_projects"], json!(true));
    db.set_settings(BTreeMap::from([
        ("show_hidden_projects".into(), json!(false)),
        ("active_project_id".into(), json!(b)),
    ])).await.unwrap();
    assert_eq!(db.get_settings().await.unwrap()["active_project_id"], json!(b));
    db.set_project_hidden(&b, true, None).await.unwrap();
    assert!(db.get_settings().await.unwrap()["active_project_id"].is_null());
    db.pool.close().await;
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn hidden_migration_preserves_existing_projects() {
    let directory = std::env::temp_dir().join(format!("orbyte-hidden-migration-{}", Uuid::new_v4()));
    let migrations = directory.join("migrations");
    std::fs::create_dir_all(&migrations).unwrap();
    std::fs::write(migrations.join("0001_initial.sql"), include_str!("../migrations/0001_initial.sql")).unwrap();
    std::fs::write(migrations.join("0002_launch_preset.sql"), include_str!("../migrations/0002_launch_preset.sql")).unwrap();
    std::fs::write(migrations.join("0003_editor_drafts.sql"), include_str!("../migrations/0003_editor_drafts.sql")).unwrap();
    let path = directory.join("old.db");
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(SqliteConnectOptions::new().filename(&path).create_if_missing(true)).await.unwrap();
    sqlx::migrate::Migrator::new(migrations.as_path()).await.unwrap().run(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO t_project(id,name,path,favorite) VALUES ('p','existing','C:/existing',1); INSERT INTO t_workspace(id,project_id) VALUES ('w','p');").execute(&pool).await.unwrap();
    pool.close().await;
    let db = Database::open(&path).await.unwrap();
    let project = &db.list_projects().await.unwrap()[0];
    assert!(!project.hidden);
    assert!(project.favorite);
    assert_eq!(project.workspace_id, "w");
    assert!(sqlx::query("UPDATE t_project SET hidden = 2 WHERE id = 'p'").execute(&db.pool).await.is_err());
    db.pool.close().await;
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn launch_preset_migration_upgrades_existing_database() {
    let directory = std::env::temp_dir().join(format!("orbyte-migration-{}", Uuid::new_v4()));
    let migrations = directory.join("migrations");
    std::fs::create_dir_all(&migrations).unwrap();
    std::fs::write(migrations.join("0001_initial.sql"), include_str!("../migrations/0001_initial.sql")).unwrap();
    let path = directory.join("old.db");
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(SqliteConnectOptions::new().filename(&path).create_if_missing(true)).await.unwrap();
    sqlx::migrate::Migrator::new(migrations.as_path()).await.unwrap().run(&pool).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 1")
        .bind(migration::original_checksum(1)).execute(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO t_project(id,name,path) VALUES ('p','old','C:\\old'); INSERT INTO t_workspace(id,project_id) VALUES ('w','p'); INSERT INTO t_terminal_config(id,workspace_id,name,shell_type,cwd) VALUES ('t','w','CMD','cmd','');").execute(&pool).await.unwrap();
    pool.close().await;
    let db = Database::open(&path).await.unwrap();
    assert_eq!(db.list_terminal_configs("w").await.unwrap()[0].launch_preset, None);
    let config = db.create_terminal_config_with_preset("p", "w", "Codex", "powershell", "", Some("codex")).await.unwrap();
    assert_eq!(db.terminal_config_with_path(&config.id).await.unwrap().0.launch_preset.as_deref(), Some("codex"));
    assert!(db.create_terminal_config_with_preset("p", "w", "bad", "powershell", "", Some("arbitrary")).await.is_err());
    assert!(sqlx::query("UPDATE t_terminal_config SET launch_preset = 'arbitrary' WHERE id = 't'").execute(&db.pool).await.is_err());
    db.pool.close().await;
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn original_migration_comments_preserve_data_and_history_on_reopen() {
    let directory = std::env::temp_dir().join(format!("orbyte-legacy-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("legacy.db");
    let db = Database::open(&path).await.unwrap();
    let project = db.create_project(input(r"C:\legacy", None)).await.unwrap();
    let workspace = db.list_projects().await.unwrap()[0].workspace_id.clone();
    let terminal = db.create_terminal_config_with_preset(
        &project, &workspace, "Codex", "powershell", "", Some("codex"),
    ).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET checksum = CASE version WHEN 1 THEN ? WHEN 2 THEN ? END WHERE version IN (1, 2)")
        .bind(migration::original_checksum(1)).bind(migration::original_checksum(2))
        .execute(&db.pool).await.unwrap();
    db.pool.close().await;

    let db = Database::open(&path).await.unwrap();
    assert_eq!(db.list_projects().await.unwrap()[0].id, project);
    assert_eq!(db.list_terminal_configs(&workspace).await.unwrap(), vec![terminal]);
    let checksums: Vec<Vec<u8>> = sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version <= 2 ORDER BY version")
        .fetch_all(&db.pool).await.unwrap();
    assert_eq!(checksums, vec![migration::original_checksum(1), migration::original_checksum(2)]);
    db.pool.close().await;
    let db = Database::open(&path).await.unwrap();
    assert_eq!(db.list_projects().await.unwrap().len(), 1);
    db.pool.close().await;
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn crlf_migrations_preserve_data_and_history_on_upgrade() {
    for applied_count in [1_i64, 2] {
        let directory = std::env::temp_dir().join(format!("orbyte-crlf-{}", Uuid::new_v4()));
        let migrations = directory.join("migrations");
        std::fs::create_dir_all(&migrations).unwrap();
        std::fs::write(
            migrations.join("0001_initial.sql"),
            include_str!("../migrations/0001_initial.sql").replace("\r\n", "\n").replace('\n', "\r\n"),
        ).unwrap();
        if applied_count == 2 {
            std::fs::write(
                migrations.join("0002_launch_preset.sql"),
                include_str!("../migrations/0002_launch_preset.sql").replace("\r\n", "\n").replace('\n', "\r\n"),
            ).unwrap();
        }
        let path = directory.join("legacy.db");
        let pool = SqlitePoolOptions::new().max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(&path).create_if_missing(true))
            .await.unwrap();
        sqlx::migrate::Migrator::new(migrations.as_path()).await.unwrap().run(&pool).await.unwrap();
        sqlx::raw_sql("INSERT INTO t_project(id,name,path) VALUES ('p','old','C:\\old'); INSERT INTO t_workspace(id,project_id) VALUES ('w','p'); INSERT INTO t_terminal_config(id,workspace_id,name,shell_type,cwd) VALUES ('t','w','CMD','cmd','');")
            .execute(&pool).await.unwrap();
        let original_checksums: Vec<Vec<u8>> = sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool).await.unwrap();
        pool.close().await;

        for _ in 0..2 {
            let db = Database::open(&path).await.unwrap();
            assert_eq!(db.list_projects().await.unwrap()[0].id, "p");
            let terminals = db.list_terminal_configs("w").await.unwrap();
            assert_eq!(terminals.len(), 1);
            assert_eq!(terminals[0].id, "t");
            assert_eq!(terminals[0].launch_preset, None);
            let checksums: Vec<Vec<u8>> = sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version <= ? ORDER BY version")
                .bind(applied_count).fetch_all(&db.pool).await.unwrap();
            assert_eq!(checksums, original_checksums);
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
                .fetch_one(&db.pool).await.unwrap();
            assert_eq!(count, 7);
            db.pool.close().await;
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[tokio::test]
async fn migration_compatibility_rejects_unknown_checksums_and_failed_migrations() {
    let db = memory().await;
    sqlx::query("UPDATE _sqlx_migrations SET checksum = CASE version WHEN 1 THEN ? ELSE X'00' END")
        .bind(migration::original_checksum(1)).execute(&db.pool).await.unwrap();
    assert!(matches!(migration::run(&db.pool).await, Err(sqlx::migrate::MigrateError::VersionMismatch(2))));
    let checksum: Vec<u8> = sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 1")
        .fetch_one(&db.pool).await.unwrap();
    assert_eq!(checksum, migration::original_checksum(1));

    sqlx::query("UPDATE _sqlx_migrations SET checksum = X'00' WHERE version = 1")
        .execute(&db.pool).await.unwrap();
    assert!(matches!(migration::run(&db.pool).await, Err(sqlx::migrate::MigrateError::VersionMismatch(1))));

    sqlx::query("UPDATE _sqlx_migrations SET success = FALSE WHERE version = 1")
        .execute(&db.pool).await.unwrap();
    assert!(matches!(migration::run(&db.pool).await, Err(sqlx::migrate::MigrateError::Dirty(1))));
    db.pool.close().await;
}

#[tokio::test]
async fn terminal_configs_validate_ownership_paths_and_follow_project_changes() {
    let db = memory().await;
    let id = db.create_project(input(r"C:\项目 A", None)).await.unwrap();
    let other = db.create_project(input(r"C:\项目 B", None)).await.unwrap();
    let projects = db.list_projects().await.unwrap();
    let workspace = &projects.iter().find(|p| p.id == id).unwrap().workspace_id;
    assert!(db
        .create_terminal_config(&other, workspace, "CMD", "cmd", "")
        .await
        .is_err());
    assert!(db
        .create_terminal_config(&id, workspace, "CMD", "bash", "")
        .await
        .is_err());
    assert!(db
        .create_terminal_config(&id, workspace, "CMD", "cmd", "../escape")
        .await
        .is_err());
    assert!(db
        .list_terminal_configs(workspace)
        .await
        .unwrap()
        .is_empty());
    let config = db
        .create_terminal_config(&id, workspace, "CMD", "cmd", "src/中文 空格")
        .await
        .unwrap();
    assert_eq!(
        db.list_terminal_configs(workspace).await.unwrap(),
        vec![config.clone()]
    );
    assert_eq!(
        db.terminal_config_with_path(&config.id).await.unwrap().1,
        r"C:\项目 A"
    );
    assert_eq!(
        db.workspaces_of_configs(&[config.id.clone(), "missing".into()])
            .await
            .unwrap(),
        vec![workspace.clone()]
    );
    assert!(db.workspaces_of_configs(&[]).await.unwrap().is_empty());
    db.update_project(&id, input(r"C:\移动后的项目", None))
        .await
        .unwrap();
    assert_eq!(
        db.terminal_config_with_path(&config.id).await.unwrap().1,
        r"C:\移动后的项目"
    );
    assert_eq!(
        db.list_project_terminal_configs(&id).await.unwrap(),
        vec![config.clone()]
    );
    assert!(db
        .list_project_terminal_configs(&other)
        .await
        .unwrap()
        .is_empty());
    db.delete_terminal_config(&config.id).await.unwrap();
    assert!(db.terminal_config_with_path(&config.id).await.is_err());
    assert!(db.delete_terminal_config(&config.id).await.is_err());
    assert_eq!(db.list_projects().await.unwrap().len(), 2);
}

#[tokio::test]
async fn terminal_configs_survive_database_reopen() {
    let path = std::env::temp_dir().join(format!("orbyte-terminal-{}.db", Uuid::new_v4()));
    let db = Database::open(&path).await.unwrap();
    let id = db.create_project(input(r"C:\项目", None)).await.unwrap();
    let workspace = db.list_projects().await.unwrap()[0].workspace_id.clone();
    let config = db
        .create_terminal_config(&id, &workspace, "PowerShell", "powershell", "")
        .await
        .unwrap();
    db.pool.close().await;
    let reopened = Database::open(&path).await.unwrap();
    assert_eq!(
        reopened.list_terminal_configs(&workspace).await.unwrap(),
        vec![config]
    );
    db.delete_project(&id).await.unwrap_err(); // 已关闭的连接不可继续写入。
    reopened.delete_project(&id).await.unwrap();
    assert!(reopened
        .list_terminal_configs(&workspace)
        .await
        .unwrap()
        .is_empty());
    reopened.pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn creates_default_workspace_rejects_duplicates_and_rolls_back_invalid_group() {
    let db = memory().await;
    let id = db.create_project(input(r"C:\项目 A", None)).await.unwrap();
    let projects = db.list_projects().await.unwrap();
    assert_eq!(projects[0].id, id);
    assert!(!projects[0].workspace_id.is_empty());
    assert!(db
        .create_project(input(r"c:\项目 a", None))
        .await
        .unwrap_err()
        .contains("重复"));
    assert!(db
        .create_project(input(r"C:\项目 B", Some("missing")))
        .await
        .is_err());
    assert_eq!(db.list_projects().await.unwrap().len(), 1);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM t_workspace")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn deleting_active_project_cascades_and_recovers_selection() {
    let db = memory().await;
    let a = db.create_project(input(r"C:\A", None)).await.unwrap();
    let b = db.create_project(input(r"C:\B", None)).await.unwrap();
    let workspace = db.list_projects().await.unwrap()[0].workspace_id.clone();
    sqlx::query("INSERT INTO t_terminal_config(id, workspace_id, name, shell_type) VALUES ('terminal', ?, 'CMD', 'cmd')").bind(&workspace).execute(&db.pool).await.unwrap();
    sqlx::query("INSERT INTO t_workspace_layout(id, workspace_id, schema_version, layout_json) VALUES ('layout', ?, 1, '{}')").bind(&workspace).execute(&db.pool).await.unwrap();
    db.set_setting("active_project_id", json!(a)).await.unwrap();
    db.delete_project(&a).await.unwrap();
    let orphan_count: i64 = sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM t_terminal_config) + (SELECT COUNT(*) FROM t_workspace_layout)").fetch_one(&db.pool).await.unwrap();
    assert_eq!(orphan_count, 0);
    assert_eq!(
        db.get_settings().await.unwrap()["active_project_id"],
        json!(b)
    );
    db.delete_project(&b).await.unwrap();
    assert!(db.get_settings().await.unwrap()["active_project_id"].is_null());
    assert!(db
        .set_setting("active_project_id", json!("missing"))
        .await
        .is_err());
}

#[tokio::test]
async fn group_restrictions_and_atomic_cross_group_sorting() {
    let db = memory().await;
    let g1 = db.create_group("财务").await.unwrap();
    let g2 = db.create_group("开发").await.unwrap();
    let a = db.create_project(input(r"C:\A", Some(&g1))).await.unwrap();
    let b = db.create_project(input(r"C:\B", None)).await.unwrap();
    assert!(db.delete_group(&g1).await.is_err());
    assert!(db
        .reorder_projects(vec![(a.clone(), None), (a.clone(), None)])
        .await
        .is_err());
    assert!(db.reorder_projects(vec![(a.clone(), None)]).await.is_err());
    assert!(db
        .reorder_projects(vec![(b.clone(), Some("missing".into())), (a.clone(), None)])
        .await
        .is_err());
    assert_eq!(
        db.list_projects().await.unwrap()[0].group_id,
        Some(g1.clone())
    );
    db.reorder_projects(vec![
        (b.clone(), Some(g2.clone())),
        (a.clone(), Some(g2.clone())),
    ])
    .await
    .unwrap();
    let ordered = db.list_projects().await.unwrap();
    assert_eq!(ordered[0].id, b);
    assert_eq!(ordered[1].id, a);
    assert!(ordered.iter().all(|p| p.group_id == Some(g2.clone())));
    db.update_group(&g2, "产品").await.unwrap();
    db.reorder_groups(vec![g2.clone(), g1.clone()])
        .await
        .unwrap();
    assert_eq!(db.list_groups().await.unwrap()[0].name, "产品");
    db.delete_group(&g1).await.unwrap();
}

#[tokio::test]
async fn open_file_shortcut_migration_preserves_legacy_binding_on_reopen() {
    let directory = std::env::temp_dir().join(format!("orbyte-shortcut-migration-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("old.db");
    let pool = SqlitePoolOptions::new().max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&path).create_if_missing(true))
        .await.unwrap();
    let mut legacy = sqlx::migrate!("./migrations");
    legacy.migrations.to_mut().retain(|migration| migration.version < 5);
    legacy.run(&pool).await.unwrap();
    let shortcuts = json!({
        "quickOpen": [{"code":"KeyP","ctrlKey":true,"altKey":false,"shiftKey":false,"metaKey":false}],
        "newTerminal": []
    });
    sqlx::query("INSERT INTO t_app_setting(key, value_json) VALUES ('shortcuts', ?)")
        .bind(shortcuts.to_string()).execute(&pool).await.unwrap();
    pool.close().await;

    let db = Database::open(&path).await.unwrap();
    let mut expected = shortcuts;
    expected["openFile"] = json!([]);
    assert_eq!(db.get_settings().await.unwrap()["shortcuts"], expected);
    db.pool.close().await;
    let db = Database::open(&path).await.unwrap();
    assert_eq!(db.get_settings().await.unwrap()["shortcuts"], expected);
    db.set_setting("shortcuts", expected.clone()).await.unwrap();
    db.pool.close().await;
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn open_file_shortcut_migration_only_changes_conflicting_implicit_defaults() {
    let db = memory().await;
    // Exercise conflicting overrides from every shortcut scope.
    let key = json!({"code":"KeyP","ctrlKey":true,"altKey":false,"shiftKey":false,"metaKey":false});
    // Apply the migration to a separate table copy so each case can share one SQL batch.
    sqlx::raw_sql("CREATE TEMP TABLE shortcut_cases (key TEXT, value_json TEXT);")
        .execute(&db.pool).await.unwrap();
    let mut values = Vec::new();
    for command in ["quickOpen", "newTerminal", "terminalCopy", "editorSave"] {
        let mut before = json!({});
        before[command] = json!([key.clone()]);
        let mut after = before.clone();
        after["openFile"] = json!([]);
        values.push((before, after));
    }
    for unchanged in [
        json!({}),
        json!({"quickOpen": []}),
        json!({"quickOpen": [{"code":"KeyP","ctrlKey":true,"altKey":false,"shiftKey":true,"metaKey":false}]}),
        json!({"openFile": []}),
        json!({"openFile": [key.clone()]}),
        json!({"openFile": [{"code":"KeyO","ctrlKey":true,"altKey":false,"shiftKey":false,"metaKey":false}], "quickOpen": [key]}),
    ] {
        values.push((unchanged.clone(), unchanged));
    }
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("INSERT INTO shortcut_cases(key, value_json) ");
    query.push_values(&values, |mut row, (before, _)| {
        row.push_bind("shortcuts").push_bind(before.to_string());
    });
    query.build().execute(&db.pool).await.unwrap();
    let sql = include_str!("../migrations/0005_open_file_shortcut.sql").replace("t_app_setting", "shortcut_cases");
    sqlx::raw_sql(&sql).execute(&db.pool).await.unwrap();
    let results: Vec<String> = sqlx::query_scalar("SELECT value_json FROM shortcut_cases ORDER BY rowid")
        .fetch_all(&db.pool).await.unwrap();
    for (result, (_, expected)) in results.iter().zip(&values) {
        let actual: Value = serde_json::from_str(result).unwrap();
        assert_eq!(&actual, expected);
        validate_setting("shortcuts", &actual).unwrap();
    }
}

#[tokio::test]
async fn rename_shortcut_migration_preserves_legacy_binding_on_reopen() {
    let directory = std::env::temp_dir().join(format!("orbyte-rename-shortcut-migration-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("old.db");
    let pool = SqlitePoolOptions::new().max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&path).create_if_missing(true))
        .await.unwrap();
    let mut legacy = sqlx::migrate!("./migrations");
    legacy.migrations.to_mut().retain(|migration| migration.version < 6);
    legacy.run(&pool).await.unwrap();
    let shortcuts = json!({
        "projectSearch": [{"code":"F2","ctrlKey":false,"altKey":false,"shiftKey":false,"metaKey":false}],
        "newTerminal": []
    });
    sqlx::query("INSERT INTO t_app_setting(key, value_json) VALUES ('shortcuts', ?)")
        .bind(shortcuts.to_string()).execute(&pool).await.unwrap();
    pool.close().await;

    let db = Database::open(&path).await.unwrap();
    let mut expected = shortcuts;
    expected["rename"] = json!([]);
    assert_eq!(db.get_settings().await.unwrap()["shortcuts"], expected);
    db.pool.close().await;
    let db = Database::open(&path).await.unwrap();
    assert_eq!(db.get_settings().await.unwrap()["shortcuts"], expected);
    db.set_setting("shortcuts", expected.clone()).await.unwrap();
    db.pool.close().await;
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn rename_shortcut_migration_only_changes_conflicting_implicit_defaults() {
    let db = memory().await;
    // Exercise conflicting overrides from every shortcut scope.
    let key = json!({"code":"F2","ctrlKey":false,"altKey":false,"shiftKey":false,"metaKey":false});
    // Apply the migration to a separate table copy so each case can share one SQL batch.
    sqlx::raw_sql("CREATE TEMP TABLE shortcut_cases (key TEXT, value_json TEXT);")
        .execute(&db.pool).await.unwrap();
    let mut values = Vec::new();
    for command in ["projectSearch", "newTerminal", "terminalCopy", "editorSave"] {
        let mut before = json!({});
        before[command] = json!([key.clone()]);
        let mut after = before.clone();
        after["rename"] = json!([]);
        values.push((before, after));
    }
    for modifier in ["ctrlKey", "altKey", "shiftKey", "metaKey"] {
        let mut modified = key.clone();
        modified[modifier] = json!(true);
        let unchanged = json!({"projectSearch": [modified]});
        values.push((unchanged.clone(), unchanged));
    }
    for unchanged in [
        json!({}),
        json!({"quickOpen": []}),
        json!({"rename": []}),
        json!({"rename": [key.clone()]}),
        json!({"rename": [{"code":"KeyO","ctrlKey":true,"altKey":false,"shiftKey":false,"metaKey":false}], "quickOpen": [key]}),
    ] {
        values.push((unchanged.clone(), unchanged));
    }
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("INSERT INTO shortcut_cases(key, value_json) ");
    query.push_values(&values, |mut row, (before, _)| {
        row.push_bind("shortcuts").push_bind(before.to_string());
    });
    query.build().execute(&db.pool).await.unwrap();
    let sql = include_str!("../migrations/0006_rename_shortcut.sql").replace("t_app_setting", "shortcut_cases");
    sqlx::raw_sql(&sql).execute(&db.pool).await.unwrap();
    let results: Vec<String> = sqlx::query_scalar("SELECT value_json FROM shortcut_cases ORDER BY rowid")
        .fetch_all(&db.pool).await.unwrap();
    for (result, (_, expected)) in results.iter().zip(&values) {
        let actual: Value = serde_json::from_str(result).unwrap();
        assert_eq!(&actual, expected);
        validate_setting("shortcuts", &actual).unwrap();
    }
}

#[tokio::test]
async fn settings_batch_is_atomic_and_preserves_active_project_validation() {
    use std::collections::BTreeMap;
    let db = memory().await;
    let before = db.get_settings().await.unwrap();
    assert_eq!(before["shortcuts"], json!({}));
    assert!(db.set_settings(BTreeMap::from([
        ("theme".into(), json!("light")),
        ("shortcuts".into(), json!({"unknown": []})),
    ])).await.is_err());
    assert_eq!(db.get_settings().await.unwrap(), before);
    assert!(db.set_settings(BTreeMap::from([
        ("theme".into(), json!("light")),
        ("font_size".into(), json!(0)),
    ])).await.is_err());
    assert_eq!(db.get_settings().await.unwrap(), before);
    assert!(db.set_settings(BTreeMap::from([
        ("theme".into(), json!("light")),
        ("active_project_id".into(), json!("missing")),
    ])).await.is_err());
    assert_eq!(db.get_settings().await.unwrap(), before);
    db.set_settings(BTreeMap::from([
        ("theme".into(), json!("light")),
        ("font_size".into(), json!(20)),
        ("scrollback".into(), json!(1000)),
    ])).await.unwrap();
    let after = db.get_settings().await.unwrap();
    assert_eq!(after["theme"], json!("light"));
    assert_eq!(after["font_size"], json!(20));
    assert_eq!(after["scrollback"], json!(1000));
    assert_eq!(after["active_project_id"], before["active_project_id"]);
}

#[tokio::test]
async fn legacy_settings_receive_workspace_border_defaults() {
    let db = memory().await;
    db.set_setting("theme", json!("light")).await.unwrap();
    let settings = db.get_settings().await.unwrap();
    assert_eq!(settings["workspace_border_color"], Value::Null);
    assert_eq!(settings["workspace_border_intensity"], json!(100));
    assert_eq!(settings["theme"], json!("light"));
    assert!(db.set_settings(std::collections::BTreeMap::from([
        ("workspace_border_color".into(), json!("#AABBCC")),
        ("workspace_border_intensity".into(), json!(101)),
    ])).await.is_err());
    assert_eq!(db.get_settings().await.unwrap(), settings);
}

#[tokio::test]
async fn settings_and_projects_survive_reopening() {
    let path = std::env::temp_dir().join(format!("orbyte-test-{}.db", Uuid::new_v4()));
    let db = Database::open(&path).await.unwrap();
    assert_eq!(db.get_settings().await.unwrap()["confirm_close_terminal"], json!(true));
    db.set_setting("confirm_close_terminal", json!(false)).await.unwrap();
    let group = db.create_group("财务项目").await.unwrap();
    let a = db
        .create_project(input(r"C:\财务 A", Some(&group)))
        .await
        .unwrap();
    let b = db
        .create_project(input(r"C:\财务 B", Some(&group)))
        .await
        .unwrap();
    db.update_project(
        &a,
        ProjectInput {
            name: "财务项目 A".into(),
            favorite: false,
            ..input(r"C:\财务 A", Some(&group))
        },
    )
    .await
    .unwrap();
    db.reorder_projects(vec![
        (b.clone(), Some(group.clone())),
        (a.clone(), Some(group.clone())),
    ])
    .await
    .unwrap();
    db.set_setting("active_project_id", json!(b)).await.unwrap();
    db.set_setting("font_size", json!(18)).await.unwrap();
    db.set_setting("theme", json!("light")).await.unwrap();
    db.set_settings(std::collections::BTreeMap::from([
        ("workspace_border_color".into(), json!("#AABBCC")),
        ("workspace_border_intensity".into(), json!(45)),
    ])).await.unwrap();
    let shortcuts = json!({"newTerminal": [], "terminalPasteAuto": [{"code":"KeyY","ctrlKey":true,"altKey":false,"shiftKey":true,"metaKey":false}]});
    db.set_setting("shortcuts", shortcuts.clone()).await.unwrap();
    db.pool.close().await;
    let reopened = Database::open(&path).await.unwrap();
    assert_eq!(reopened.list_groups().await.unwrap()[0].name, "财务项目");
    let projects = reopened.list_projects().await.unwrap();
    assert_eq!(projects[0].id, b);
    assert_eq!(projects[1].name, "财务项目 A");
    assert!(!projects[1].favorite);
    let settings = reopened.get_settings().await.unwrap();
    assert_eq!(settings["active_project_id"], json!(b));
    assert_eq!(settings["font_size"], json!(18));
    assert_eq!(settings["confirm_close_terminal"], json!(false));
    assert_eq!(settings["theme"], json!("light"));
    assert_eq!(settings["shortcuts"], shortcuts);
    assert_eq!(settings["workspace_border_color"], json!("#AABBCC"));
    assert_eq!(settings["workspace_border_intensity"], json!(45));
    reopened.set_settings(std::collections::BTreeMap::from([
        ("workspace_border_color".into(), Value::Null),
        ("workspace_border_intensity".into(), json!(100)),
    ])).await.unwrap();
    let reset = reopened.get_settings().await.unwrap();
    assert_eq!(reset["workspace_border_color"], Value::Null);
    assert_eq!(reset["workspace_border_intensity"], json!(100));
    assert_eq!(reset["theme"], settings["theme"]);
    assert_eq!(reset["font_size"], settings["font_size"]);
    reopened.set_setting("shortcuts", json!({})).await.unwrap();
    assert_eq!(reopened.get_settings().await.unwrap()["shortcuts"], json!({}));
    reopened.pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn invalid_settings_and_stale_active_project() {
    let db = memory().await;
    assert!(db.set_setting("unknown", json!("x")).await.is_err());
    assert!(db.set_setting("font_size", json!("18")).await.is_err());
    let id = db.create_project(input(r"C:\A", None)).await.unwrap();
    sqlx::query(
        "INSERT INTO t_app_setting(key, value_json) VALUES ('active_project_id', '\"missing\"')",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        db.get_settings().await.unwrap()["active_project_id"],
        json!(id)
    );
}
