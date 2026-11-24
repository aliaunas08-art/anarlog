//! ASPC C4.2: a meeting whose title, notes and transcript carry a unique
//! marker is run through the Rust paths of this plugin that log or return
//! errors (permanent delete, retention, backup and a refused restore, an
//! audit entry with content). The marker must appear in no captured log
//! line and no error string. Other parts of C4.2 live next to their code:
//! `provider_errors_never_carry_meeting_content` (Engine),
//! `meeting_content_markers_never_reach_analytics` (analytics sink),
//! `denials_never_carry_the_statement_text` (webview SQL),
//! `content-marker.test.ts` (desktop error reports and console forwarding).

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use anlg_db_core::{Db, DbOpenOptions, DbStorage};

const MARKER: &str = "zqxcontentmarker4417";
const ID: &str = "6f1c2d3e-0000-4000-8000-00000000e333";
const OLD: &str = "6f1c2d3e-0000-4000-8000-00000000e444";

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Every log line of this test binary from every thread (including blocking
/// tasks) at the level `app.log` records (`info`, as in `plugins/tracing`).
fn captured_logs() -> &'static Captured {
    static CAPTURED: OnceLock<Captured> = OnceLock::new();
    CAPTURED.get_or_init(|| {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
        captured
    })
}

async fn library(path: &Path) -> Db {
    let db = Db::open(DbOpenOptions {
        storage: DbStorage::Local(path),
        cloudsync_enabled: false,
        journal_mode_wal: true,
        foreign_keys: true,
        max_connections: Some(4),
    })
    .await
    .unwrap();
    anlg_db_app::prepare_schema(&db).await.unwrap();
    db
}

async fn seed(pool: &sqlx::SqlitePool, id: &str, created_at: &str) {
    sqlx::query("INSERT INTO sessions (id, title, created_at) VALUES (?, ?, ?)")
        .bind(id)
        .bind(format!("Title {MARKER}"))
        .bind(created_at)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO session_documents (id, session_id, kind, body_format, body)
         VALUES (?, ?, 'note', 'markdown', ?)",
    )
    .bind(id)
    .bind(id)
    .bind(format!("Notes {MARKER}"))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO transcripts (id, session_id, words_json) VALUES (?, ?, ?)")
        .bind(format!("{id}-t"))
        .bind(id)
        .bind(format!("[{{\"text\":\"{MARKER}\"}}]"))
        .execute(pool)
        .await
        .unwrap();
}

fn assert_clean(what: &str, text: &str) {
    assert!(
        !text.contains(MARKER),
        "{what} carries meeting content: {text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn meeting_content_reaches_no_log_or_error() {
    let logs = captured_logs();
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let sessions = data.join("sessions");
    std::fs::create_dir_all(sessions.join(ID)).unwrap();
    std::fs::write(sessions.join(ID).join("_memo.md"), format!("# {MARKER}")).unwrap();
    let db = library(&data.join("app.db")).await;
    let pool = db.pool();
    seed(pool, ID, "2026-11-16T10:00:00.000Z").await;
    seed(pool, OLD, "2020-01-01T10:00:00.000Z").await;
    tracing::info!("marker test started");

    // Retention: the old meeting is past a 30-day period.
    let now = chrono::Utc::now();
    let expired = crate::retention::expired_session_ids(pool, now, 30, &HashSet::new())
        .await
        .unwrap();
    let deleted = crate::retention::soft_delete_for_retention(pool, &expired, now)
        .await
        .unwrap();
    assert_eq!(
        deleted,
        vec![OLD.to_string()],
        "retention deleted the old meeting"
    );

    // Permanent delete of a soft-deleted meeting (D2.2), past its undo window.
    sqlx::query("UPDATE sessions SET deleted_at = '2026-01-01T00:00:00.000Z' WHERE id = ?")
        .bind(ID)
        .execute(pool)
        .await
        .unwrap();
    let mut ids = deleted.clone();
    ids.push(ID.to_string());
    let (purged, _) = crate::purge::purge_sessions_in_db(pool, &ids)
        .await
        .unwrap();
    assert_eq!(purged.len(), ids.len(), "every marked meeting was purged");

    // Backup of the library, then restores that fail.
    seed(
        pool,
        "6f1c2d3e-0000-4000-8000-00000000e555",
        "2026-11-16T11:00:00.000Z",
    )
    .await;
    let archive = root.path().join("library.anarlogbackup");
    crate::backup::write_backup(pool, Some(&sessions), &archive, "1.0.0")
        .await
        .unwrap();
    let missing = root.path().join("missing.anarlogbackup");
    let errors = [
        crate::backup::stage_restore(&missing, &data, Some(&sessions))
            .await
            .unwrap_err(),
        {
            let hostile = root.path().join("hostile.anarlogbackup");
            let mut builder = tar::Builder::new(std::fs::File::create(&hostile).unwrap());
            let mut header = tar::Header::new_gnu();
            let name = format!("{MARKER}/notes.md");
            header.set_path(&name).unwrap();
            header.set_size(6);
            header.set_cksum();
            builder.append(&header, &b"notes!"[..]).unwrap();
            builder.finish().unwrap();
            crate::backup::stage_restore(&hostile, &data, Some(&sessions))
                .await
                .unwrap_err()
        },
    ];
    for error in &errors {
        assert_clean("restore error", error);
    }

    // An audit entry with content is refused without echoing it.
    let audit = anlg_audit_log::open_append_only(&data.join("audit.db"))
        .await
        .unwrap();
    let refused = audit
        .append(
            anlg_audit_log::NewEntry::new("meeting.viewed", "meeting", ID)
                .id("title", format!("Title {MARKER}")),
        )
        .await
        .unwrap_err();
    assert_clean("audit error", &refused.to_string());
    let audit_bytes = std::fs::read(data.join("audit.db")).unwrap_or_default();
    assert!(
        !audit_bytes
            .windows(MARKER.len())
            .any(|window| window == MARKER.as_bytes())
    );

    drop(db);
    let text = String::from_utf8_lossy(&logs.0.lock().unwrap()).into_owned();
    assert!(text.contains("marker test started"), "logs are captured");
    assert_clean("log output", &text);
}
