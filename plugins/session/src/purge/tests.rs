use std::collections::HashSet;
use std::path::Path;

use anlg_db_core::{Db, DbOpenOptions, DbStorage};
use sqlx::SqlitePool;

use super::*;

const MARKER: &str = "zebrapurgemarker7731";
const ID: &str = "6f1c2d3e-0000-4000-8000-00000000d222";
const OTHER: &str = "6f1c2d3e-0000-4000-8000-00000000c111";

fn contains(bytes: &[u8]) -> bool {
    bytes
        .windows(MARKER.len())
        .any(|window| window == MARKER.as_bytes())
}

/// Every file under `dir` whose bytes contain the marker.
fn files_with_marker(dir: &Path) -> Vec<String> {
    let mut hits = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return hits;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            hits.extend(files_with_marker(&path));
        } else if std::fs::read(&path).is_ok_and(|bytes| contains(&bytes)) {
            hits.push(path.display().to_string());
        }
    }
    hits
}

async fn open(path: &Path) -> Db {
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

async fn seed(pool: &SqlitePool, id: &str, marker: &str) {
    let m = marker;
    for statement in [
        format!(
            "INSERT INTO sessions (id, title, event_json) VALUES ('{id}', 'Title {m}', '{{\"title\":\"{m}\"}}')"
        ),
        format!(
            "INSERT INTO session_documents (id, session_id, kind, body_format, body) VALUES ('{id}', '{id}', 'note', 'markdown', 'First note {m}')"
        ),
        format!("UPDATE session_documents SET body = 'Edited note {m}' WHERE id = '{id}'"),
        format!(
            "INSERT INTO session_documents (id, session_id, kind, body_format, body) VALUES ('{id}-chat', '{id}', 'meeting_chat', 'markdown', 'Chat {m}')"
        ),
        format!(
            "INSERT INTO transcripts (id, session_id, words_json) VALUES ('{id}-t', '{id}', '[{{\"text\":\"{m}\"}}]')"
        ),
        format!("INSERT INTO transcript_live_state (transcript_id) VALUES ('{id}-t')"),
        format!(
            "INSERT INTO transcript_live_deltas (id, transcript_id, sequence, delta_json) VALUES ('{id}-d', '{id}-t', 0, '{{\"new_words\":[\"{m}\"]}}')"
        ),
        format!(
            "INSERT INTO session_attachments (id, session_id, filename) VALUES ('{id}-f', '{id}', 'file-{m}.pdf')"
        ),
        format!("INSERT INTO chat_groups (id, title) VALUES ('{id}-g', 'Ask {m}')"),
        format!(
            "INSERT INTO chat_messages (id, chat_group_id, content, metadata_json) VALUES ('{id}-msg', '{id}-g', 'Answer {m}', '{{\"askScope\":\"meeting\",\"askMeetingId\":\"{id}\"}}')"
        ),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement.clone()))
            .execute(pool)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

fn seed_files(vault_sessions: &Path, cache: &Path, id: &str, marker: &str) {
    let dir = vault_sessions.join("Projects").join(id);
    std::fs::create_dir_all(dir.join("audio-recovery")).unwrap();
    std::fs::create_dir_all(dir.join("attachments")).unwrap();
    std::fs::write(dir.join("audio.wav"), format!("RIFF {marker}")).unwrap();
    std::fs::write(dir.join("audio-recovery/0001.part"), marker).unwrap();
    std::fs::write(dir.join("attachments/file.pdf"), format!("%PDF {marker}")).unwrap();
    std::fs::write(
        dir.join("_meta.json"),
        format!("{{\"title\":\"{marker}\"}}"),
    )
    .unwrap();
    std::fs::create_dir_all(cache.join("audio-peaks")).unwrap();
    std::fs::write(cache.join(format!("audio-peaks/{id}.json")), marker).unwrap();
}

/// D2.2: a meeting whose title, transcript, note, chat and attachment carry
/// a marker leaves the marker nowhere in `app.db`, its `-wal`, the vault or
/// the caches after the purge. (The search index part is tested in
/// `plugins/tantivy`: `purged_sessions_leave_no_bytes_in_the_index_directory`.)
#[tokio::test]
async fn a_purged_meeting_leaves_no_marker_in_the_database_wal_vault_or_cache() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let vault_sessions = dir.path().join("vault/sessions");
    let cache = dir.path().join("cache");
    let db = open(&db_path).await;
    seed(db.pool(), ID, MARKER).await;
    seed(db.pool(), OTHER, "keptmeetingword").await;
    seed_files(&vault_sessions, &cache, ID, MARKER);
    seed_files(&vault_sessions, &cache, OTHER, "keptmeetingword");

    let on_disk = |name: &str| std::fs::read(dir.path().join(name)).unwrap_or_default();
    assert!(
        contains(&on_disk("app.db")) || contains(&on_disk("app.db-wal")),
        "the fixture must reach the database files"
    );

    // Inside the undo window nothing is removed.
    let (purged, _) = purge_sessions_in_db(db.pool(), &[ID.to_string()])
        .await
        .unwrap();
    assert!(purged.is_empty());

    let mut conn = db.pool().acquire().await.unwrap();
    for table in [
        "session_documents",
        "transcripts",
        "session_participants",
        "session_tags",
        "action_items",
        "session_attachments",
    ] {
        anlg_db_app::update_session_child_tombstone(
            &mut conn,
            table,
            ID,
            "2026-11-16T00:00:00Z",
            false,
        )
        .await
        .unwrap();
    }
    anlg_db_app::update_sessions_tombstone(&mut conn, ID, "2026-11-16T00:00:00Z", false)
        .await
        .unwrap();
    drop(conn);

    let (purged, wal_truncated) = purge_sessions_in_db(db.pool(), &[ID.to_string()])
        .await
        .unwrap();
    assert_eq!(purged.len(), 1);
    assert!(!purged[0].purge.kept_tombstone);
    assert!(wal_truncated);
    let ids: HashSet<String> = [ID.to_string()].into();
    let removed = purge_session_files(Some(&vault_sessions), Some(&cache), &ids);
    assert_eq!(removed, 2);

    assert_eq!(
        std::fs::metadata(dir.path().join("app.db-wal")).map_or(0, |m| m.len()),
        0,
        "the WAL is truncated"
    );
    let db_hits = files_with_marker(dir.path())
        .into_iter()
        .filter(|path| path.contains("app.db"))
        .collect::<Vec<_>>();
    assert!(db_hits.is_empty(), "{db_hits:?}");
    assert_eq!(files_with_marker(&vault_sessions), Vec::<String>::new());
    assert_eq!(files_with_marker(&cache), Vec::<String>::new());

    // The other meeting is untouched.
    assert!(
        vault_sessions
            .join("Projects")
            .join(OTHER)
            .join("audio.wav")
            .exists()
    );
    let kept: i64 =
        sqlx::query_scalar("SELECT count(*) FROM session_documents WHERE session_id = ?")
            .bind(OTHER)
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(kept, 2);
    db.pool().close().await;
}

#[test]
fn crafted_ids_are_refused() {
    assert!(is_purgeable_id(ID));
    for bad in ["", "..", "a", "../x", "sessions/x", "*", "abc def12"] {
        assert!(!is_purgeable_id(bad), "{bad}");
    }
}
