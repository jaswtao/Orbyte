//! Compatibility for migration comments and line endings changed during open-source preparation.
//! Author: jaswtao
//! Date: 2026-10-20

use sqlx::{
    migrate::{Migrate, MigrateError},
    SqlitePool,
};

// (version, published LF SHA-384, accepted historical SHA-384). Both sides must match exactly.
const COMPATIBLE_CHECKSUMS: &[(i64, &str, &str)] = &[
    (
        1,
        "6473bd11456d24be326c916e35aeb0a78d63429e8cee6676009bf7c8cc1e4efcbccec2fb0392403a4fb3a1749f7ad9b5",
        "ce80c7bba71b56f2d02659ae4b1a5adeb95ba7085659fda4c00be25cc45608f6fe3b9def185c330b35284a7c88891a74",
    ),
    (
        2,
        "7f6048f581927c96a16c690edaa2a05ade9ed2d916272d09a39eaf63b4bd5d80c6e83d1c092a5d2a165981e3a06e9b91",
        "98b9ee126e477a254f7c6d796c9a6d40a2cada8162e2d5ff4ef7ec2a325be5010b35c69af6f9379edf3e9202a0b4d445",
    ),
    // Published SQL checked out with core.autocrlf=true before SQL was pinned to LF.
    (
        1,
        "6473bd11456d24be326c916e35aeb0a78d63429e8cee6676009bf7c8cc1e4efcbccec2fb0392403a4fb3a1749f7ad9b5",
        "cd1f1d8569fe88664c04be81b4476034534cfd61a6530914d32610a939ca786b3dba42c7cc1463fb4b209a60579255fa",
    ),
    (
        2,
        "7f6048f581927c96a16c690edaa2a05ade9ed2d916272d09a39eaf63b4bd5d80c6e83d1c092a5d2a165981e3a06e9b91",
        "8cb5dfcc00af5059e0e4f4435c3e95bf10841125f293c7e61ab7aac363bde96409ca4eb2b8e430b0a3eab58cc6a5264d",
    ),
];

fn checksum_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) async fn run(pool: &SqlitePool) -> Result<(), MigrateError> {
    let mut migrator = sqlx::migrate!("./migrations");
    let mut connection = pool.acquire().await?;
    connection.ensure_migrations_table().await?;
    let applied = connection.list_applied_migrations().await?;

    // Accept only verified comment and line-ending changes without rewriting database history.
    // Unknown checksums and any future edits still go through SQLx's normal validation.
    for migration in migrator.migrations.to_mut() {
        if let Some(previous) = applied.iter().find(|entry| entry.version == migration.version) {
            let current = checksum_hex(&migration.checksum);
            let old = checksum_hex(&previous.checksum);
            if COMPATIBLE_CHECKSUMS
                .iter()
                .any(|&(version, published, original)| {
                    version == migration.version && published == current && original == old
                })
            {
                migration.checksum = previous.checksum.clone();
            }
        }
    }

    migrator.run(&mut *connection).await
}

#[cfg(test)]
pub(super) fn original_checksum(version: i64) -> Vec<u8> {
    let hex = COMPATIBLE_CHECKSUMS
        .iter()
        .find(|entry| entry.0 == version)
        .unwrap()
        .2;
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
        .collect()
}
