//! ASPC C1 interim: the single-workspace guard on webview SQL fails closed.
//!
//! While every content row belongs to the local workspace (the CloudSync
//! binding) or none, webview SQL works as before. As soon as a row of another
//! workspace exists, every content table is denied to the webview, whatever
//! form the statement takes, until the other workspace's rows are gone.

use std::sync::Arc;

use anlg_db_core::{AuthorizedConnection, Db, WORKSPACE_SCOPED_TABLES, WebviewSqlAuthorizer};
use db_execute::{DbExecutor, ProxyQueryMethod, TransactionStatement};
use serde_json::json;
use sqlx::Connection;

struct Fixture {
    _dir: tempfile::TempDir,
    db: Arc<Db>,
    webview: DbExecutor,
}

async fn fixture(binding: Option<&str>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.db");
    let db = Db::open(anlg_db_core::DbOpenOptions {
        storage: anlg_db_core::DbStorage::Local(&path),
        cloudsync_enabled: false,
        journal_mode_wal: true,
        foreign_keys: false,
        max_connections: Some(4),
    })
    .await
    .unwrap();
    let mut schema = String::from(
        "CREATE TABLE app_settings (id TEXT PRIMARY KEY, value_json TEXT NOT NULL);
         CREATE TABLE transcript_live_state (transcript_id TEXT PRIMARY KEY, words TEXT);
         INSERT INTO transcript_live_state VALUES ('t-local', 'live words');
         CREATE VIEW session_titles AS SELECT title FROM sessions;",
    );
    for table in WORKSPACE_SCOPED_TABLES {
        schema.push_str(&format!(
            "CREATE TABLE {table} (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT '',
               session_id TEXT NOT NULL DEFAULT '', title TEXT NOT NULL DEFAULT '');"
        ));
    }
    sqlx::raw_sql(sqlx::AssertSqlSafe(schema))
        .execute(db.pool())
        .await
        .unwrap();
    if let Some(binding) = binding {
        sqlx::query("INSERT INTO app_settings VALUES ('cloudsync_workspace_binding', ?)")
            .bind(json!({ "workspace_id": binding, "account_user_id": null }).to_string())
            .execute(db.pool())
            .await
            .unwrap();
    }
    sqlx::raw_sql(
        "INSERT INTO sessions (id, workspace_id, title) VALUES
           ('s-local', 'local-ws', 'LOCAL-MARKER'), ('s-unassigned', '', 'UNASSIGNED');
         INSERT INTO transcripts (id, workspace_id, session_id, title)
           VALUES ('t-local', 'local-ws', 's-local', 'LOCAL-WORDS');",
    )
    .execute(db.pool())
    .await
    .unwrap();
    let db = Arc::new(db);
    let authorizer = WebviewSqlAuthorizer::new();
    authorizer.enable_workspace_guard();
    let webview = DbExecutor::for_webview(Arc::clone(&db), authorizer);
    Fixture {
        _dir: dir,
        db,
        webview,
    }
}

async fn add_foreign_rows(db: &Db) {
    sqlx::raw_sql(
        "INSERT INTO sessions (id, workspace_id, title) VALUES ('s-foreign', 'team-ws', 'FOREIGN-MARKER');
         INSERT INTO transcripts (id, workspace_id, session_id, title)
           VALUES ('t-foreign', 'team-ws', 's-foreign', 'FOREIGN-WORDS');",
    )
    .execute(db.pool())
    .await
    .unwrap();
}

fn denied<T: std::fmt::Debug>(result: db_execute::Result<T>) -> bool {
    match result {
        Err(error) => {
            let message = error.to_string();
            message.contains("not authorized") || message.contains("prohibited")
        }
        Ok(value) => {
            eprintln!("allowed: {value:?}");
            false
        }
    }
}

async fn select(webview: &DbExecutor, sql: &str) -> db_execute::Result<Vec<serde_json::Value>> {
    webview.execute(sql.to_string(), Vec::new()).await
}

#[tokio::test]
async fn single_workspace_reads_and_writes_work() {
    let f = fixture(Some("local-ws")).await;
    let rows = select(&f.webview, "SELECT id FROM sessions ORDER BY id")
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![json!({"id": "s-local"}), json!({"id": "s-unassigned"})]
    );
    f.webview
        .execute(
            "UPDATE sessions SET title = 'edited' WHERE id = ?".into(),
            vec![json!("s-local")],
        )
        .await
        .unwrap();
    f.webview
        .execute_proxy(
            "INSERT INTO sessions (id, workspace_id, title) VALUES (?, 'local-ws', 'new')
             ON CONFLICT(id) DO UPDATE SET title = excluded.title"
                .into(),
            vec![json!("s-new")],
            ProxyQueryMethod::Run,
        )
        .await
        .unwrap();
    f.webview
        .execute_transaction(vec![TransactionStatement {
            sql: "INSERT INTO transcripts (id, workspace_id, session_id) VALUES ('t-new', '', 's-new')".into(),
            params: Vec::new(),
            expected_rows_affected: Some(1),
        }])
        .await
        .unwrap();
    // Tables a read reaches without naming them (a view, a content table
    // without workspace_id) work after the full census.
    select(&f.webview, "SELECT title FROM session_titles")
        .await
        .unwrap();
    select(&f.webview, "SELECT words FROM transcript_live_state")
        .await
        .unwrap();
    let explain = select(&f.webview, "EXPLAIN QUERY PLAN SELECT * FROM sessions").await;
    assert!(
        explain.is_ok(),
        "EXPLAIN QUERY PLAN works in single-workspace mode"
    );
}

/// Every way of reaching the second workspace's rows is denied.
#[tokio::test]
async fn a_second_workspace_denies_every_content_path() {
    let f = fixture(Some("local-ws")).await;
    add_foreign_rows(&f.db).await;

    for sql in [
        "SELECT * FROM sessions",
        "SELECT * FROM main.sessions",
        "SELECT title FROM sessions WHERE id = 's-local'",
        "SELECT s.title, t.title FROM sessions s JOIN transcripts t ON t.session_id = s.id",
        "SELECT (SELECT title FROM sessions WHERE workspace_id = 'team-ws')",
        "SELECT 1 WHERE EXISTS (SELECT 1 FROM sessions)",
        "WITH x AS (SELECT * FROM sessions) SELECT * FROM x",
        "WITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM r WHERE n < 2) SELECT title FROM sessions, r",
        "SELECT count(*) FROM transcripts",
        "SELECT words FROM transcript_live_state",
        "SELECT title FROM session_titles",
        "SELECT title FROM humans, session_titles",
        "EXPLAIN QUERY PLAN SELECT * FROM sessions",
        "UPDATE sessions SET title = 'x'",
        "DELETE FROM sessions",
        "DELETE FROM sessions RETURNING title",
        "UPDATE sessions SET title = 'x' RETURNING title",
        "INSERT INTO sessions (id, workspace_id) VALUES ('s-x', 'local-ws') RETURNING id",
        "INSERT INTO sessions (id, workspace_id) SELECT id || '-copy', 'local-ws' FROM main.sessions",
    ] {
        assert!(denied(select(&f.webview, sql).await), "{sql}");
    }
    assert!(denied(
        f.webview
            .execute_proxy(
                "SELECT title FROM sessions".into(),
                Vec::new(),
                ProxyQueryMethod::All
            )
            .await
    ));
    assert!(denied(
        f.webview
            .execute_transaction(vec![TransactionStatement {
                sql: "UPDATE sessions SET title = 'x'".into(),
                params: Vec::new(),
                expected_rows_affected: None,
            }])
            .await
    ));

    // Schema and settings stay readable: they hold no meeting content.
    select(
        &f.webview,
        "SELECT name FROM sqlite_master WHERE type = 'table'",
    )
    .await
    .unwrap();
    select(&f.webview, "SELECT name FROM pragma_table_info('sessions')")
        .await
        .unwrap();
    select(&f.webview, "PRAGMA table_info(sessions)")
        .await
        .unwrap();
    select(&f.webview, "SELECT value_json FROM app_settings")
        .await
        .unwrap();

    // Nothing was changed by the denied writes.
    let titles: Vec<String> = sqlx::query_scalar("SELECT title FROM sessions ORDER BY id")
        .fetch_all(f.db.pool())
        .await
        .unwrap();
    assert_eq!(titles, ["FOREIGN-MARKER", "LOCAL-MARKER", "UNASSIGNED"]);

    // Resolved: once the other workspace's rows are gone, access returns.
    sqlx::raw_sql("DELETE FROM sessions WHERE workspace_id = 'team-ws'; DELETE FROM transcripts WHERE workspace_id = 'team-ws';")
        .execute(f.db.pool())
        .await
        .unwrap();
    select(&f.webview, "SELECT * FROM sessions").await.unwrap();
}

/// Writes are confined: a webview write that would create another
/// workspace's rows is rolled back.
#[tokio::test]
async fn webview_writes_cannot_create_another_workspace() {
    let f = fixture(Some("local-ws")).await;
    for sql in [
        "INSERT INTO sessions (id, workspace_id) VALUES ('s-evil', 'team-ws')",
        "INSERT INTO sessions (id, workspace_id) VALUES ('s-evil', 'team-ws') RETURNING id",
        "UPDATE sessions SET workspace_id = 'team-ws' WHERE id = 's-local'",
        "UPDATE transcripts SET workspace_id = 'team-ws'",
        "WITH x AS (SELECT 1) INSERT INTO humans (id, workspace_id) SELECT 'h', 'team-ws' FROM x",
    ] {
        assert!(denied(select(&f.webview, sql).await), "{sql}");
    }
    assert!(denied(
        f.webview
            .execute_proxy(
                "INSERT INTO sessions (id, workspace_id) VALUES ('s-evil', 'team-ws')".into(),
                Vec::new(),
                ProxyQueryMethod::Run,
            )
            .await
    ));
    assert!(denied(
        f.webview
            .execute_transaction(vec![
                TransactionStatement {
                    sql: "UPDATE sessions SET title = 'kept?' WHERE id = 's-local'".into(),
                    params: Vec::new(),
                    expected_rows_affected: Some(1),
                },
                TransactionStatement {
                    sql: "INSERT INTO tags (id, workspace_id) VALUES ('tag', 'team-ws')".into(),
                    params: Vec::new(),
                    expected_rows_affected: Some(1),
                },
            ])
            .await
    ));
    let foreign: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM sessions WHERE workspace_id <> 'local-ws' AND workspace_id <> '')
              + (SELECT count(*) FROM transcripts WHERE workspace_id <> 'local-ws')
              + (SELECT count(*) FROM humans) + (SELECT count(*) FROM tags)
              + (SELECT count(*) FROM sessions WHERE title = 'kept?')",
    )
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(
        foreign, 0,
        "every write that made a second workspace was rolled back"
    );
    // Still in single-workspace mode afterwards.
    select(&f.webview, "SELECT * FROM sessions").await.unwrap();
}

/// The webview cannot end the guard's transaction to escape its snapshot.
#[tokio::test]
async fn webview_cannot_control_transactions() {
    let f = fixture(Some("local-ws")).await;
    for sql in [
        "COMMIT",
        "ROLLBACK",
        "BEGIN",
        "COMMIT; SELECT * FROM sessions",
        "END",
    ] {
        assert!(denied(select(&f.webview, sql).await), "{sql}");
    }
    assert!(denied(
        f.webview
            .execute_transaction(vec![TransactionStatement {
                sql: "COMMIT".into(),
                params: Vec::new(),
                expected_rows_affected: None,
            }])
            .await
    ));
    // The pool connections are still usable afterwards.
    select(&f.webview, "SELECT * FROM sessions").await.unwrap();
    sqlx::query("BEGIN IMMEDIATE; COMMIT;")
        .execute(f.db.pool())
        .await
        .ok();
}

/// No local workspace (no binding): every non-empty workspace counts as
/// foreign, so content is denied.
#[tokio::test]
async fn no_binding_fails_closed() {
    let f = fixture(None).await;
    assert!(denied(select(&f.webview, "SELECT * FROM sessions").await));
}

/// A census failure denies content: here the binding's table is gone, and
/// in the full census (writes) a missing content table.
#[tokio::test]
async fn census_failure_fails_closed() {
    let f = fixture(Some("local-ws")).await;
    sqlx::query("DROP TABLE voiceprint_exemplars")
        .execute(f.db.pool())
        .await
        .unwrap();
    assert!(denied(
        select(
            &f.webview,
            "UPDATE sessions SET title = 'x' WHERE id = 's-local'"
        )
        .await
    ));
    sqlx::query("DROP TABLE app_settings")
        .execute(f.db.pool())
        .await
        .unwrap();
    assert!(denied(select(&f.webview, "SELECT * FROM sessions").await));
}

/// The census and the statement share one snapshot: a foreign row committed
/// by another connection after the census is not visible to the statement.
#[tokio::test]
async fn census_and_statement_share_one_snapshot() {
    let f = fixture(Some("local-ws")).await;
    let authorizer = WebviewSqlAuthorizer::new();
    authorizer.enable_workspace_guard();
    let mut authorized = AuthorizedConnection::acquire(f.db.pool(), &authorizer)
        .await
        .unwrap();
    let guard = authorized.workspace_guard().unwrap();
    guard.allow_transaction_control(true);
    let mut transaction = authorized.connection().begin_with("BEGIN").await.unwrap();
    guard.allow_transaction_control(false);
    guard.check(&mut transaction).await;

    add_foreign_rows(&f.db).await;

    let titles: Vec<String> = sqlx::query_scalar("SELECT title FROM sessions ORDER BY id")
        .fetch_all(&mut *transaction)
        .await
        .unwrap();
    assert_eq!(titles, ["LOCAL-MARKER", "UNASSIGNED"]);
    guard.allow_transaction_control(true);
    transaction.commit().await.unwrap();
    guard.allow_transaction_control(false);
    authorized.release().await.unwrap();

    // The next statement sees the foreign row in its census and is denied.
    assert!(denied(select(&f.webview, "SELECT * FROM sessions").await));
}

/// Without the guard turned on (Phase 1/2a behaviour and other tests), the
/// census does not run.
#[tokio::test]
async fn guard_is_off_unless_enabled() {
    let f = fixture(Some("local-ws")).await;
    add_foreign_rows(&f.db).await;
    let plain = DbExecutor::for_webview(Arc::clone(&f.db), WebviewSqlAuthorizer::new());
    assert_eq!(
        select(&plain, "SELECT * FROM sessions")
            .await
            .unwrap()
            .len(),
        3
    );
}
