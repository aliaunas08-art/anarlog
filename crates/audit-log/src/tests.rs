use std::sync::Arc;
use std::time::Duration;

use anlg_local_accounts::{MemoryStore, SecretStore};
use sqlx::SqlitePool;

use super::*;

struct Fixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    store: Arc<MemoryStore>,
    writer: AuditWriter,
    reader: AuditReader,
}

async fn fixture(entries: usize) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(AUDIT_DB_FILENAME);
    let store = Arc::new(MemoryStore::default());
    let (writer, reader) = open(&path, store.clone()).await.unwrap();
    writer.reconcile_head().await.unwrap();
    for i in 0..entries {
        writer
            .append(
                NewEntry::new(actions::MEETING_VIEWED, "meeting", format!("meeting-{i}"))
                    .count("n", i as i64),
            )
            .await
            .unwrap();
    }
    writer.flush().await;
    Fixture {
        _dir: dir,
        path,
        store,
        writer,
        reader,
    }
}

/// A second, raw connection to the file, as an attacker with file access
/// would use, with the append-only triggers dropped.
async fn tamper(path: &std::path::Path, statements: &[&str]) {
    let pool = SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(path))
        .await
        .unwrap();
    sqlx::query("DROP TRIGGER audit_entries_no_update")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DROP TRIGGER audit_entries_no_delete")
        .execute(&pool)
        .await
        .unwrap();
    for statement in statements {
        sqlx::query(sqlx::AssertSqlSafe(*statement))
            .execute(&pool)
            .await
            .unwrap();
    }
    pool.close().await;
}

#[tokio::test]
async fn appends_chain_and_verify() {
    let f = fixture(5).await;
    let page = f.reader.list(None, 2).await.unwrap();
    assert_eq!(page.total, 5);
    assert_eq!(page.entries[0].seq, 5);
    assert_eq!(page.next_before_seq, Some(4));
    let older = f.reader.list(page.next_before_seq, 10).await.unwrap();
    assert_eq!(older.entries.len(), 3);
    assert_eq!(older.entries.last().unwrap().prev_hash, GENESIS_HASH);
    assert_eq!(older.entries[0].hash, page.entries[1].prev_hash);

    let report = f.reader.verify().await.unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.entries, 5);
    assert!(report.head_checked);
    // The Keychain holds the latest seq:hash.
    let head = f.store.get(head::HEAD_KEY).unwrap().unwrap();
    assert_eq!(head, format!("5:{}", page.entries[0].hash));
}

#[tokio::test]
async fn update_and_delete_fail_through_the_triggers() {
    let f = fixture(2).await;
    let pool =
        SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&f.path))
            .await
            .unwrap();
    let update = sqlx::query("UPDATE audit_entries SET object_id = 'x' WHERE seq = 1")
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(update.to_string().contains("append-only"), "{update}");
    let delete = sqlx::query("DELETE FROM audit_entries WHERE seq = 2")
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(delete.to_string().contains("append-only"), "{delete}");
    pool.close().await;
    assert!(f.reader.verify().await.unwrap().ok);
}

#[tokio::test]
async fn an_edited_entry_is_the_first_bad_one() {
    let f = fixture(5).await;
    tamper(
        &f.path,
        &["UPDATE audit_entries SET object_id = 'meeting-other' WHERE seq = 3"],
    )
    .await;
    let report = f.reader.verify().await.unwrap();
    assert!(!report.ok);
    assert_eq!(report.first_bad_seq, Some(3));
    assert!(report.reason.unwrap().contains("edited"));
}

#[tokio::test]
async fn a_deleted_entry_is_reported_missing() {
    let f = fixture(5).await;
    tamper(&f.path, &["DELETE FROM audit_entries WHERE seq = 2"]).await;
    let report = f.reader.verify().await.unwrap();
    assert!(!report.ok);
    assert_eq!(report.first_bad_seq, Some(2));
    assert!(report.reason.unwrap().contains("missing"));
}

#[tokio::test]
async fn reordered_entries_point_to_the_first_moved_one() {
    let f = fixture(5).await;
    tamper(
        &f.path,
        &[
            "UPDATE audit_entries SET seq = -1 WHERE seq = 3",
            "UPDATE audit_entries SET seq = 3 WHERE seq = 4",
            "UPDATE audit_entries SET seq = 4 WHERE seq = -1",
        ],
    )
    .await;
    let report = f.reader.verify().await.unwrap();
    assert!(!report.ok);
    assert_eq!(report.first_bad_seq, Some(3));
}

#[tokio::test]
async fn truncation_is_caught_by_the_keychain_head() {
    let f = fixture(5).await;
    tamper(&f.path, &["DELETE FROM audit_entries WHERE seq > 3"]).await;
    let report = f.reader.verify().await.unwrap();
    assert!(!report.ok);
    assert_eq!(report.first_bad_seq, Some(4));
    assert!(report.reason.unwrap().contains("truncated"));

    // Reopening does not reset the anchor: the head stays as evidence even
    // after more appends.
    drop(f.writer);
    drop(f.reader);
    let (writer, reader) = open(&f.path, f.store.clone()).await.unwrap();
    writer.reconcile_head().await.unwrap();
    for _ in 0..4 {
        writer
            .append(NewEntry::new(actions::APP_UNLOCKED, "app", ""))
            .await
            .unwrap();
    }
    writer.flush().await;
    assert!(
        f.store
            .get(head::HEAD_KEY)
            .unwrap()
            .unwrap()
            .starts_with("5:")
    );
    let report = reader.verify().await.unwrap();
    assert!(!report.ok);
    assert_eq!(report.first_bad_seq, Some(4));
}

#[tokio::test]
async fn a_consistently_rewritten_chain_is_caught_by_the_keychain_head() {
    let f = fixture(3).await;
    // Rewrite entry 2 and recompute every later hash, as an attacker who
    // knows the scheme would.
    let pool =
        SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&f.path))
            .await
            .unwrap();
    let mut entries: Vec<Entry> = sqlx::query_as("SELECT * FROM audit_entries ORDER BY seq")
        .fetch_all(&pool)
        .await
        .unwrap();
    pool.close().await;
    entries[1].object_id = "meeting-forged".into();
    for i in 1..entries.len() {
        entries[i].prev_hash = entries[i - 1].hash.clone();
        entries[i].hash = entries[i].compute_hash();
    }
    let statements: Vec<String> = entries[1..]
        .iter()
        .map(|e| {
            format!(
                "UPDATE audit_entries SET object_id = '{}', prev_hash = '{}', hash = '{}' WHERE seq = {}",
                e.object_id, e.prev_hash, e.hash, e.seq
            )
        })
        .collect();
    let refs: Vec<&str> = statements.iter().map(String::as_str).collect();
    tamper(&f.path, &refs).await;
    let report = f.reader.verify().await.unwrap();
    assert!(!report.ok, "{report:?}");
    assert_eq!(report.first_bad_seq, Some(3));
    assert!(report.reason.unwrap().contains("rewritten"));
}

#[tokio::test]
async fn first_run_without_a_head_stores_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(AUDIT_DB_FILENAME);
    let store = Arc::new(MemoryStore::default());
    let (writer, reader) = open(&path, store.clone()).await.unwrap();
    // Entries written before the Keychain was read are anchored once it is.
    writer
        .append(NewEntry::new(actions::APP_UNLOCKED, "app", ""))
        .await
        .unwrap();
    assert!(store.get(head::HEAD_KEY).unwrap().is_none());
    writer.reconcile_head().await.unwrap();
    writer.flush().await;
    assert!(
        store
            .get(head::HEAD_KEY)
            .unwrap()
            .unwrap()
            .starts_with("1:")
    );
    assert!(reader.verify().await.unwrap().head_checked);
}

struct SlowStore;

impl SecretStore for SlowStore {
    fn get(&self, _key: &str) -> Result<Option<String>, anlg_local_accounts::Error> {
        std::thread::sleep(Duration::from_secs(8));
        Ok(None)
    }
    fn set(&self, _key: &str, _value: &str) -> Result<(), anlg_local_accounts::Error> {
        Ok(())
    }
    fn delete(&self, _key: &str) -> Result<(), anlg_local_accounts::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn a_keychain_that_hangs_does_not_block_appends_or_verify() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(AUDIT_DB_FILENAME);
    let (writer, reader) = open(&path, Arc::new(SlowStore)).await.unwrap();
    let started = std::time::Instant::now();
    writer.reconcile_head().await.unwrap();
    writer
        .append(NewEntry::new(actions::APP_UNLOCKED, "app", ""))
        .await
        .unwrap();
    let report = reader.verify().await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(8));
    assert!(report.ok);
    assert!(!report.head_checked);
    assert!(report.head_note.unwrap().contains("Keychain"));
}

#[tokio::test]
async fn a_denial_loop_writes_one_entry() {
    let f = fixture(0).await;
    for _ in 0..1000 {
        f.writer.submit_denial(
            "app:aspc.locked:Notes:Read",
            NewEntry::new(actions::PERMISSION_DENIED, "command", "db:execute")
                .id("code", "aspc.locked"),
        );
    }
    f.writer.flush().await;
    let page = f.reader.list(None, 50).await.unwrap();
    assert_eq!(page.total, 1);
}

#[tokio::test]
async fn content_is_refused_and_nothing_is_written() {
    let f = fixture(0).await;
    let error = f
        .writer
        .append(NewEntry::new(actions::ASK_QUERY, "ask", "").id("query", "what did Bob say"))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidField(_)));
    assert_eq!(f.reader.list(None, 10).await.unwrap().total, 0);
}

#[tokio::test]
async fn the_append_only_handle_writes_without_the_keychain() {
    let f = fixture(2).await;
    let cli = open_append_only(&f.path).await.unwrap();
    cli.append(NewEntry::new(actions::MEETING_DELETED, "meeting", "m-1").source(Source::Cli))
        .await
        .unwrap();
    // The app's head lags behind the CLI's entry, which verify accepts.
    let report = f.reader.verify().await.unwrap();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.entries, 3);
    assert_eq!(
        f.reader.list(None, 1).await.unwrap().entries[0].source,
        "cli"
    );
}
