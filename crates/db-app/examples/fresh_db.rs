//! Creates a fresh `app.db` with the app's migrations and seed data at the
//! given path, for `scripts/check-secrets --fresh-db` (ASPC C3.1: a fresh
//! database holds no key).
//!
//!   cargo run --locked -p db-app --example fresh_db -- /tmp/x/app.db

use anlg_db_core::{Db, DbOpenOptions, DbStorage};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .ok_or("usage: fresh_db <path to app.db>")?;
    if path.exists() {
        return Err("refusing to touch an existing database".into());
    }
    let db = Db::open(DbOpenOptions {
        storage: DbStorage::Local(&path),
        cloudsync_enabled: false,
        journal_mode_wal: true,
        foreign_keys: true,
        max_connections: Some(1),
    })
    .await?;
    db_app::prepare_schema(&db).await?;
    // Fold the WAL into the main file so one file holds everything.
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(db.pool())
        .await?;
    println!("{}", path.display());
    Ok(())
}
