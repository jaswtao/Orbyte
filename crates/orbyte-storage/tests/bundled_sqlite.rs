// Author: jaswtao · Date: 2026-11-16

#[tokio::test]
async fn bundled_sqlite_uses_the_security_patched_engine() {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    let version: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let version: Vec<u32> = version.split('.').map(|part| part.parse().unwrap()).collect();
    assert!(version.as_slice() >= [3, 54, 1].as_slice());
    pool.close().await;
}
