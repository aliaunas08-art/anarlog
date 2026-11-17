use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use super::*;

async fn library(dir: &Path) -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(dir.join("app.db"))
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT, deleted_at TEXT);
         CREATE TABLE transcripts (id TEXT PRIMARY KEY, session_id TEXT, words TEXT);
         INSERT INTO sessions VALUES ('s1', 'Quarterly plan', NULL), ('s2', 'Hiring sync', NULL);
         INSERT INTO transcripts VALUES ('t1', 's1', 'we agreed to ship in May');",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn write(path: &Path, contents: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

async fn rows(pool: &SqlitePool) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT s.id, COALESCE(s.title, '') || '|' || COALESCE(t.words, '')
         FROM sessions s LEFT JOIN transcripts t ON t.session_id = s.id ORDER BY s.id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// C5.2: back up, wipe, restore, then every meeting and file matches.
#[tokio::test]
async fn backup_restores_into_a_clean_library() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let sessions = data.join("sessions");
    std::fs::create_dir_all(&data).unwrap();
    let pool = library(&data).await;
    write(&sessions.join("s1/audio.wav"), b"RIFF-audio-bytes");
    write(
        &sessions.join("s1/attachments/slide.pdf"),
        b"%PDF-1.7 slide",
    );
    write(&sessions.join("s2/_memo.md"), b"# Hiring sync");
    let original = rows(&pool).await;
    let archive = root.path().join("library.anarlogbackup");

    let summary = write_backup(&pool, Some(&sessions), &archive, "1.2.3")
        .await
        .unwrap();
    assert_eq!(summary.meetings, 2);
    assert_eq!(summary.files, 3);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&archive).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "the archive is private");
    }

    // Wipe: a brand-new, empty library in the same place.
    pool.close().await;
    std::fs::remove_dir_all(&data).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let empty = library(&data).await;
    sqlx::raw_sql("DELETE FROM transcripts; DELETE FROM sessions;")
        .execute(&empty)
        .await
        .unwrap();
    write(&sessions.join("new/_memo.md"), b"after the wipe");

    let staged = stage_restore(&archive, &data, Some(&sessions))
        .await
        .unwrap();
    assert_eq!(staged.meetings, 2);
    assert_eq!(staged.files, 3);
    assert!(staged.restart_required);
    // Nothing changes until the next launch.
    assert!(rows(&empty).await.is_empty());
    empty.close().await;

    let applied = apply_pending_restore(&data).unwrap().expect("applied");
    let restored = library_without_schema(&data).await;
    assert_eq!(rows(&restored).await, original);
    assert_eq!(
        std::fs::read(sessions.join("s1/audio.wav")).unwrap(),
        b"RIFF-audio-bytes"
    );
    assert_eq!(
        std::fs::read(sessions.join("s1/attachments/slide.pdf")).unwrap(),
        b"%PDF-1.7 slide"
    );
    assert_eq!(
        std::fs::read(sessions.join("s2/_memo.md")).unwrap(),
        b"# Hiring sync"
    );
    assert!(!sessions.join("new").exists());
    // The library it replaced is kept, not deleted.
    let previous = applied.previous_sessions.expect("previous sessions kept");
    assert!(previous.join("new/_memo.md").is_file());
    assert!(
        applied
            .previous_database
            .expect("previous db kept")
            .is_file()
    );
    assert!(!data.join(PENDING_DIR).exists());
    assert_eq!(apply_pending_restore(&data).unwrap(), None);
}

async fn library_without_schema(dir: &Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .connect_with(SqliteConnectOptions::new().filename(dir.join("app.db")))
        .await
        .unwrap()
}

fn archive_with(path: &Path, build: impl FnOnce(&mut tar::Builder<File>)) {
    let mut builder = tar::Builder::new(File::create(path).unwrap());
    build(&mut builder);
    builder.finish().unwrap();
}

fn regular(builder: &mut tar::Builder<File>, name: &str, data: &[u8]) {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o600);
    header.set_entry_type(tar::EntryType::Regular);
    // `set_path` refuses `..`; write the raw name like a hostile archive.
    let bytes = name.as_bytes();
    header.as_old_mut().name[..bytes.len()].copy_from_slice(bytes);
    header.set_cksum();
    builder.append(&header, data).unwrap();
}

#[tokio::test]
async fn hostile_or_foreign_archives_are_refused_and_change_nothing() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let manifest = serde_json::to_vec(&BackupManifest {
        format: BACKUP_FORMAT,
        created_at: "2026-11-16T00:00:00Z".into(),
        app_version: "1".into(),
        meetings: 0,
        files: 0,
    })
    .unwrap();

    let cases: Vec<(&str, Box<dyn FnOnce(&mut tar::Builder<File>)>)> = vec![
        (
            "escape",
            Box::new(|b| regular(b, "sessions/../../evil.sh", b"x")),
        ),
        ("absolute", Box::new(|b| regular(b, "/tmp/evil", b"x"))),
        ("unknown", Box::new(|b| regular(b, "audit.db", b"x"))),
        (
            "symlink",
            Box::new(|b| {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                b.append_link(&mut header, "sessions/link", "/etc").unwrap();
            }),
        ),
        (
            "not a database",
            Box::new({
                let manifest = manifest.clone();
                move |b| {
                    regular(b, "manifest.json", &manifest);
                    regular(b, "app.db", b"not sqlite");
                }
            }),
        ),
        (
            "newer format",
            Box::new(|b| {
                regular(
                    b,
                    "manifest.json",
                    br#"{"format":99,"created_at":"x","app_version":"x","meetings":0,"files":0}"#,
                )
            }),
        ),
    ];
    for (name, build) in cases {
        let archive = root.path().join(format!("{name}.anarlogbackup"));
        archive_with(&archive, build);
        let result = stage_restore(&archive, &data, None).await;
        assert!(result.is_err(), "{name} was accepted");
        assert!(!data.join(PENDING_DIR).exists(), "{name} staged something");
        assert!(!root.path().join("evil.sh").exists());
    }
    assert_eq!(apply_pending_restore(&data).unwrap(), None);
}
