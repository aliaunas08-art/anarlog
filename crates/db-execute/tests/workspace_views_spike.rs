//! ASPC C1 spike, NOT SHIPPED: per-connection TEMP views that shadow content
//! tables with `workspace_id = <active>`, with INSTEAD OF triggers for writes.
//!
//! Each test asserts what the view mechanism would need for webview SQL to
//! fail closed. They are ignored because several of those properties do not
//! hold; run them with `cargo test -p db-execute --test workspace_views_spike
//! -- --ignored` to see which. Results and the decision are in
//! docs/ASPC_AUDIT.md, "C1 decision".

use sqlx::{Connection, SqliteConnection};

const VIEWS: &str = "
CREATE TEMP VIEW sessions AS
  SELECT * FROM main.sessions WHERE workspace_id IN ('local-ws', '');
";

const TRIGGERS: &str = "
CREATE TEMP TRIGGER sessions_insert INSTEAD OF INSERT ON temp.sessions BEGIN
  INSERT INTO main.sessions (id, workspace_id, title) VALUES (NEW.id, 'local-ws', NEW.title);
END;
CREATE TEMP TRIGGER sessions_update INSTEAD OF UPDATE ON temp.sessions BEGIN
  UPDATE main.sessions SET title = NEW.title
  WHERE id = OLD.id AND workspace_id IN ('local-ws', '');
END;
CREATE TEMP TRIGGER sessions_delete INSTEAD OF DELETE ON temp.sessions BEGIN
  DELETE FROM main.sessions WHERE id = OLD.id AND workspace_id IN ('local-ws', '');
END;
";

async fn connection() -> SqliteConnection {
    let mut conn = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql(
        "CREATE TABLE sessions (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT '',
           title TEXT NOT NULL DEFAULT '');
         CREATE TABLE transcripts (id TEXT PRIMARY KEY, session_id TEXT, workspace_id TEXT);
         INSERT INTO sessions VALUES ('local', 'local-ws', 'LOCAL'), ('foreign', 'team-ws', 'FOREIGN');
         INSERT INTO transcripts VALUES ('t1', 'foreign', 'team-ws');",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    sqlx::raw_sql(VIEWS).execute(&mut conn).await.unwrap();
    conn
}

async fn titles(conn: &mut SqliteConnection, sql: &str) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(conn)
        .await
}

/// Holds: unqualified reads, joins, subqueries and CTEs go through the view.
#[tokio::test]
#[ignore = "C1 spike, not shipped"]
async fn unqualified_reads_are_filtered() {
    let mut conn = connection().await;
    for sql in [
        "SELECT title FROM sessions",
        "SELECT s.title FROM transcripts t JOIN sessions s ON s.id = t.session_id",
        "SELECT (SELECT title FROM sessions WHERE id = 'foreign')",
        "WITH x AS (SELECT * FROM sessions) SELECT title FROM x",
    ] {
        let rows = titles(&mut conn, sql).await.unwrap();
        assert!(!rows.iter().any(|t| t == "FOREIGN"), "{sql}: {rows:?}");
    }
}

/// Does NOT hold with the SQLite the app links: a TEMP INSTEAD OF trigger
/// may not name `main.sessions` in INSERT/UPDATE/DELETE ("qualified table
/// names are not allowed ... within triggers"), and the unqualified name
/// resolves to the view itself. Writes through views cannot reach the table.
#[tokio::test]
#[ignore = "C1 spike, not shipped: INSTEAD OF triggers cannot write main.<table>"]
async fn writes_through_the_view_are_confined() {
    let mut conn = connection().await;
    let version: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    eprintln!("linked SQLite {version}");
    sqlx::raw_sql(TRIGGERS).execute(&mut conn).await.unwrap();
    sqlx::query("UPDATE sessions SET title = 'x'")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sessions")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions (id, workspace_id, title) VALUES ('evil', 'team-ws', 'E')")
        .execute(&mut conn)
        .await
        .unwrap();
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT id, workspace_id, title FROM main.sessions ORDER BY id")
            .fetch_all(&mut conn)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            ("evil".into(), "local-ws".into(), "E".into()),
            ("foreign".into(), "team-ws".into(), "FOREIGN".into()),
        ]
    );
}

/// Does NOT hold without more work: `main.sessions` bypasses the view. It
/// needs the authorizer to deny `main`-qualified reads unless the accessor is
/// the view, which the current callback does not see (it ignores the
/// database and accessor arguments).
#[tokio::test]
#[ignore = "C1 spike, not shipped: main-qualified names bypass the view"]
async fn main_qualified_reads_are_denied() {
    let mut conn = connection().await;
    let rows = titles(&mut conn, "SELECT title FROM main.sessions").await;
    assert!(rows.is_err(), "main.sessions returned {rows:?}");
}

/// Does NOT hold: SQLite rejects UPSERT on a view ("cannot UPSERT a view"),
/// and the frontend writes sessions, documents, chats and other content with
/// `INSERT ... ON CONFLICT DO UPDATE` (e.g. `session/queries/sessions.ts`,
/// `chat/store/queries.ts`). Those writes would all fail.
#[tokio::test]
#[ignore = "C1 spike, not shipped: views reject UPSERT"]
async fn frontend_upserts_still_work() {
    let mut conn = connection().await;
    sqlx::query(
        "INSERT INTO sessions (id, title) VALUES ('local', 'renamed')
         ON CONFLICT(id) DO UPDATE SET title = excluded.title",
    )
    .execute(&mut conn)
    .await
    .unwrap();
}

/// Does NOT hold: webview SQL runs on the shared pool. A TEMP view on a pool
/// connection also shadows the table for every trusted Rust query that later
/// gets that connection (sync, claim, backfill, search indexing), silently
/// filtering them. It needs a dedicated webview connection pool (with the
/// CloudSync extension and change hooks loaded on it).
#[tokio::test]
#[ignore = "C1 spike, not shipped: TEMP views leak into trusted queries on a shared pool"]
async fn trusted_queries_on_the_same_connection_see_every_row() {
    let mut conn = connection().await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(count, 2, "an internal Rust query saw a filtered table");
}
