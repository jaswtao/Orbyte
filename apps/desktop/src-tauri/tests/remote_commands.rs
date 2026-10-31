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
            app.state(), "test".into(), "127.0.0.1".into(), Some(listener.local_addr().unwrap().port()),
            "test".into(), "password".into(), None,
        ).await.unwrap();
        let workspace = load_remote_workspace(app.state(), connection.clone()).await.unwrap();
        let config = create_remote_terminal_config(app.state(), workspace.id.clone(), "test".into()).await.unwrap();
        let batch = begin_ssh_connect(app.state(), "password".into(), Channel::new(|_| Ok(()))).unwrap();
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
