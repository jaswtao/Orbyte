//! SSH command runtime regressions. No real SSH server is required.
//!
//! Author: jaswtao
//! Date: 2026-10-31

#[allow(dead_code)]
#[path = "../src/commands/mod.rs"]
mod commands;
#[allow(dead_code)]
#[path = "../src/state.rs"]
mod state;

use commands::project::StorageState;
use commands::remote::*;
use orbyte_storage::Database;
use state::AppState;
use tauri::{ipc::Channel, test::{mock_builder, mock_context, noop_assets}, Manager};

#[test]
fn host_store_attaches_outside_a_tokio_context_and_connects_from_async_commands() {
    assert!(tokio::runtime::Handle::try_current().is_err());
    let db = tauri::async_runtime::block_on(Database::open(std::path::Path::new(":memory:"))).unwrap();
    let state = AppState::new();
    attach_host_store(&state.ssh, db.clone());
    let app = mock_builder()
        .manage(state)
        .manage(StorageState(Ok(db)))
        .build(mock_context(noop_assets())).unwrap();

    tauri::async_runtime::block_on(async {
        // A bound, silent local endpoint keeps the connection in the handshake
        // while the commands return their attempt IDs, then cancellation stops it.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let connection = create_ssh_connection(
            app.state(), app.state(), "test".into(), "127.0.0.1".into(), Some(listener.local_addr().unwrap().port()),
            "test".into(), "password".into(), None, None,
        ).await.unwrap();
        let workspace = load_remote_workspace(app.state(), connection.clone()).await.unwrap();
        let config = create_remote_terminal_config(app.state(), workspace.id.clone(), "test".into()).await.unwrap();
        let batch = begin_ssh_connect(app.state(), app.state(), connection.clone(), Channel::new(|_| Ok(()))).await.unwrap();
        let shell = connect_ssh_terminal(
            app.state(), app.state(), batch.batch_id.clone(), connection.clone(), config,
            80, 24, Channel::new(|_| Ok(())),
        ).await.unwrap();
        assert!(!shell.terminal_id.is_empty());
        let sftp = connect_sftp(app.state(), app.state(), batch.batch_id.clone(), connection, workspace.id).await.unwrap();
        assert!(!sftp.attempt_id.is_empty());
        cancel_ssh_attempt(app.state(), format!("batch:{}", batch.batch_id)).unwrap();
        app.state::<AppState>().ssh.shutdown();
    });
}

#[derive(Default)]
struct TestPasswords {
    values: std::sync::Mutex<std::collections::HashMap<String, String>>,
    failure: std::sync::atomic::AtomicU8,
}

impl orbyte_platform::credentials::PasswordStore for TestPasswords {
    fn read(&self, id: &str) -> Result<Option<zeroize::Zeroizing<String>>, String> {
        if self.failure.load(std::sync::atomic::Ordering::SeqCst) == 1 { return Err("read failed".into()); }
        Ok(self.values.lock().unwrap().get(id).cloned().map(zeroize::Zeroizing::new))
    }
    fn write(&self, id: &str, password: &str) -> Result<(), String> {
        if self.failure.load(std::sync::atomic::Ordering::SeqCst) == 2 { return Err("write failed".into()); }
        self.values.lock().unwrap().insert(id.into(), password.into());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        if self.failure.load(std::sync::atomic::Ordering::SeqCst) == 3 { return Err("delete failed".into()); }
        self.values.lock().unwrap().remove(id);
        Ok(())
    }
}

fn preset(value: &str) -> Option<PasswordChange> {
    Some(PasswordChange::Set { password: value.into() })
}

#[test]
fn saved_password_lifecycle_never_serializes_the_password() {
    use orbyte_platform::credentials::PasswordStore;
    let passwords = std::sync::Arc::new(TestPasswords::default());
    let mut state = AppState::new();
    state.ssh_passwords = passwords.clone();
    let db = tauri::async_runtime::block_on(Database::open(std::path::Path::new(":memory:"))).unwrap();
    let app = mock_builder().manage(state).manage(StorageState(Ok(db.clone()))).build(mock_context(noop_assets())).unwrap();
    tauri::async_runtime::block_on(async {
        let id = create_ssh_connection(app.state(), app.state(), "lab".into(), "Example.COM".into(), None, "root".into(), "password".into(), None, preset("  private-test-value  ")).await.unwrap();
        let view = list_ssh_connections(app.state(), app.state()).await.unwrap();
        assert!(view[0].has_saved_password);
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("private-test-value"));
        assert!(json.contains("hasSavedPassword"));
        assert_eq!(passwords.read(&id).unwrap().unwrap().as_str(), "  private-test-value  ");
        update_ssh_connection(app.state(), app.state(), id.clone(), "renamed".into(), "example.com".into(), None, "root".into(), "password".into(), None, None).await.unwrap();
        assert!(passwords.read(&id).unwrap().is_some());
        update_ssh_connection(app.state(), app.state(), id.clone(), "renamed".into(), "example.com".into(), None, "other".into(), "password".into(), None, None).await.unwrap();
        assert!(passwords.read(&id).unwrap().is_none());
        update_ssh_connection(app.state(), app.state(), id.clone(), "renamed".into(), "example.com".into(), None, "other".into(), "password".into(), None, preset("replacement")).await.unwrap();
        assert_eq!(passwords.read(&id).unwrap().unwrap().as_str(), "replacement");
        update_ssh_connection(app.state(), app.state(), id.clone(), "renamed".into(), "example.com".into(), None, "other".into(), "password".into(), None, Some(PasswordChange::Clear)).await.unwrap();
        assert!(passwords.read(&id).unwrap().is_none());
        update_ssh_connection(app.state(), app.state(), id.clone(), "renamed".into(), "example.com".into(), None, "other".into(), "password".into(), None, preset("replacement")).await.unwrap();
        update_ssh_connection(app.state(), app.state(), id.clone(), "renamed".into(), "example.com".into(), None, "other".into(), "privateKey".into(), Some("C:/keys/id_ed25519".into()), None).await.unwrap();
        assert!(passwords.read(&id).unwrap().is_none());
        assert!(update_ssh_connection(app.state(), app.state(), id.clone(), "renamed".into(), "example.com".into(), None, "other".into(), "privateKey".into(), Some("C:/keys/id_ed25519".into()), preset("forbidden")).await.is_err());
        delete_ssh_connection(app.state(), app.state(), id.clone()).await.unwrap();
        assert!(db.ssh_connection(&id).await.is_err());
        assert!(passwords.read(&id).unwrap().is_none());
    });
}

#[test]
fn credential_store_failures_are_reported_without_success_or_orphan_connections() {
    use std::sync::atomic::Ordering::SeqCst;
    use orbyte_platform::credentials::PasswordStore;
    let passwords = std::sync::Arc::new(TestPasswords::default());
    let mut state = AppState::new();
    state.ssh_passwords = passwords.clone();
    let db = tauri::async_runtime::block_on(Database::open(std::path::Path::new(":memory:"))).unwrap();
    let app = mock_builder().manage(state).manage(StorageState(Ok(db.clone()))).build(mock_context(noop_assets())).unwrap();
    tauri::async_runtime::block_on(async {
        passwords.failure.store(2, SeqCst);
        assert!(create_ssh_connection(app.state(), app.state(), "lab".into(), "example.com".into(), None, "root".into(), "password".into(), None, preset("value")).await.is_err());
        assert!(db.list_ssh_connections().await.unwrap().is_empty());
        passwords.failure.store(0, SeqCst);
        let id = create_ssh_connection(app.state(), app.state(), "lab".into(), "example.com".into(), None, "root".into(), "password".into(), None, preset("original")).await.unwrap();
        passwords.failure.store(1, SeqCst);
        assert!(list_ssh_connections(app.state(), app.state()).await.is_err());
        assert!(begin_ssh_connect(app.state(), app.state(), id.clone(), Channel::new(|_| Ok(()))).await.is_err());
        passwords.failure.store(2, SeqCst);
        assert!(update_ssh_connection(app.state(), app.state(), id.clone(), "changed".into(), "example.com".into(), None, "root".into(), "password".into(), None, preset("new")).await.is_err());
        assert_eq!(db.ssh_connection(&id).await.unwrap().name, "lab");
        assert_eq!(passwords.read(&id).unwrap().unwrap().as_str(), "original");
        passwords.failure.store(3, SeqCst);
        assert!(delete_ssh_connection(app.state(), app.state(), id.clone()).await.is_err());
        assert!(db.ssh_connection(&id).await.is_ok());
        passwords.failure.store(0, SeqCst);
        delete_ssh_connection(app.state(), app.state(), id.clone()).await.unwrap();
        assert!(passwords.read(&id).unwrap().is_none());
    });
}

#[test]
fn login_saves_only_when_requested_and_rejects_expired_or_cross_connection_batches() {
    use orbyte_platform::credentials::PasswordStore;
    let passwords = std::sync::Arc::new(TestPasswords::default());
    let mut state = AppState::new();
    state.ssh_passwords = passwords.clone();
    let db = tauri::async_runtime::block_on(Database::open(std::path::Path::new(":memory:"))).unwrap();
    attach_host_store(&state.ssh, db.clone());
    let app = mock_builder().manage(state).manage(StorageState(Ok(db))).build(mock_context(noop_assets())).unwrap();
    tauri::async_runtime::block_on(async {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let id = create_ssh_connection(app.state(), app.state(), "lab".into(), "127.0.0.1".into(), Some(listener.local_addr().unwrap().port()), "root".into(), "password".into(), None, None).await.unwrap();
        let workspace = load_remote_workspace(app.state(), id.clone()).await.unwrap();
        let batch = begin_ssh_connect(app.state(), app.state(), id.clone(), Channel::new(|_| Ok(()))).await.unwrap();
        assert!(connect_sftp(app.state(), app.state(), batch.batch_id.clone(), "other".into(), workspace.id.clone()).await.is_err());
        connect_sftp(app.state(), app.state(), batch.batch_id.clone(), id.clone(), workspace.id).await.unwrap();
        seal_ssh_connect(app.state(), batch.batch_id.clone()).unwrap();
        submit_ssh_credential(app.state(), app.state(), batch.batch_id.clone(), "temporary".into(), None).await.unwrap();
        assert!(passwords.read(&id).unwrap().is_none());
        passwords.failure.store(2, std::sync::atomic::Ordering::SeqCst);
        assert!(submit_ssh_credential(app.state(), app.state(), batch.batch_id.clone(), "saved".into(), Some(true)).await.is_err());
        passwords.failure.store(0, std::sync::atomic::Ordering::SeqCst);
        submit_ssh_credential(app.state(), app.state(), batch.batch_id.clone(), "saved".into(), Some(true)).await.unwrap();
        assert_eq!(passwords.read(&id).unwrap().unwrap().as_str(), "saved");
        cancel_ssh_attempt(app.state(), format!("batch:{}", batch.batch_id)).unwrap();
        assert!(submit_ssh_credential(app.state(), app.state(), batch.batch_id, "late".into(), Some(true)).await.is_err());
        assert_eq!(passwords.read(&id).unwrap().unwrap().as_str(), "saved");
        app.state::<AppState>().ssh.shutdown();
    });
}

#[test]
#[ignore = "requires the isolated OpenSSH fixture and Windows Credential Manager"]
fn saved_system_password_authenticates_terminals_and_sftp_without_frontend_credentials() {
    use orbyte_platform::credentials::{PasswordStore, SystemPasswordStore};
    assert_eq!(std::env::var("ORBYTE_SSH_PROBE").as_deref(), Ok("1"));
    let password = zeroize::Zeroizing::new(std::env::var("ORBYTE_SSH_PASSWORD").expect("fixture password"));
    let db = tauri::async_runtime::block_on(Database::open(std::path::Path::new(":memory:"))).unwrap();
    let state = AppState::new();
    attach_host_store(&state.ssh, db.clone());
    let app = mock_builder().manage(state).manage(StorageState(Ok(db))).build(mock_context(noop_assets())).unwrap();
    struct Cleanup { id: String, ssh: std::sync::Arc<orbyte_ssh::SshHub> }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.ssh.shutdown();
            SystemPasswordStore.delete(&self.id).expect("clean up isolated test credential");
        }
    }
    tauri::async_runtime::block_on(async {
        let id = create_ssh_connection(app.state(), app.state(), "probe".into(), "127.0.0.1".into(), Some(2222), "orbyte".into(), "password".into(), None, preset(&password)).await.unwrap();
        let ssh = app.state::<AppState>().ssh.clone();
        let _cleanup = Cleanup { id: id.clone(), ssh: ssh.clone() };
        let (tx, rx) = std::sync::mpsc::channel();
        let events = Channel::new(move |body| {
            let tauri::ipc::InvokeResponseBody::Json(json) = body else { panic!("expected JSON event") };
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            if value["type"] == "challenge" {
                ssh.answer_challenge(value["challengeId"].as_str().unwrap(), "once").unwrap();
            } else if value["type"] == "credentialRequired" {
                tx.send(Err("saved password unexpectedly requested from frontend".to_string())).unwrap();
            } else if value["type"] == "attempt" && value["state"] == "failed" {
                tx.send(Err(value["message"].to_string())).unwrap();
            } else if value["type"] == "attempt" && value["state"] == "ready" {
                tx.send(Ok(())).unwrap();
            }
            Ok(())
        });
        let workspace = load_remote_workspace(app.state(), id.clone()).await.unwrap();
        let first = create_remote_terminal_config(app.state(), workspace.id.clone(), "first".into()).await.unwrap();
        let second = create_remote_terminal_config(app.state(), workspace.id.clone(), "second".into()).await.unwrap();
        let batch = begin_ssh_connect(app.state(), app.state(), id.clone(), events).await.unwrap();
        connect_ssh_terminal(app.state(), app.state(), batch.batch_id.clone(), id.clone(), first, 80, 24, Channel::new(|_| Ok(()))).await.unwrap();
        // Force the first authentication to finish before registering the remaining requests.
        rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap().unwrap();
        connect_ssh_terminal(app.state(), app.state(), batch.batch_id.clone(), id.clone(), second, 80, 24, Channel::new(|_| Ok(()))).await.unwrap();
        connect_sftp(app.state(), app.state(), batch.batch_id.clone(), id.clone(), workspace.id).await.unwrap();
        seal_ssh_connect(app.state(), batch.batch_id).unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap().unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap().unwrap();
        assert!(SystemPasswordStore.read(&id).unwrap().is_some());
    });
}
