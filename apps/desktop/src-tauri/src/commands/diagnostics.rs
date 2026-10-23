//! Accept bounded diagnostic metadata, never IPC payloads. Author: jaswtao · Date: 2026-10-23
use super::CmdResult;

#[tauri::command]
pub fn report_app_diagnostic(
    event: String,
    command: String,
    outcome: String,
    elapsed_ms: u64,
    context: String,
) -> CmdResult<()> {
    if !matches!(
        event.as_str(),
        "command_started"
            | "command_finished"
            | "window_error"
            | "unhandled_rejection"
            | "vue_error"
            | "frontend_ready"
    ) || !matches!(outcome.as_str(), "pending" | "ok" | "error")
        || command.len() > 96
        || !command
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || context.len() > 1024
    {
        return Err("Invalid diagnostic metadata".into());
    }
    if outcome == "error" {
        log::error!(target: "orbyte_frontend", "event={event} command={command} outcome={outcome} elapsed_ms={elapsed_ms} {context}");
    } else if elapsed_ms >= 1000 {
        log::warn!(target: "orbyte_frontend", "event={event} command={command} outcome={outcome} elapsed_ms={elapsed_ms} {context}");
    } else {
        log::info!(target: "orbyte_frontend", "event={event} command={command} outcome={outcome} elapsed_ms={elapsed_ms} {context}");
    }
    Ok(())
}
