#![forbid(unsafe_code)]

mod error;
mod query;

use std::sync::Arc;

use anlg_db_core::{AuthorizedConnection, WebviewSqlAuthorizer, WorkspaceGuard};
use sqlx::{Connection, SqliteConnection};

pub use error::{Error, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxyQueryMethod {
    Run,
    All,
    Get,
    Values,
}

impl std::str::FromStr for ProxyQueryMethod {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "run" => Ok(Self::Run),
            "all" => Ok(Self::All),
            "get" => Ok(Self::Get),
            "values" => Ok(Self::Values),
            _ => Err(Error::InvalidQueryMethod(s.to_string())),
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ProxyQueryResult {
    pub rows: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionStatement {
    pub sql: String,
    pub params: Vec<serde_json::Value>,
    pub expected_rows_affected: Option<u64>,
}

/// Runs raw SQL. An executor built with [`DbExecutor::for_webview`] runs every
/// statement under the webview SQL authorizer (ASPC A2.4); one built with
/// [`DbExecutor::new`] is for trusted internal callers.
#[derive(Clone)]
pub struct DbExecutor {
    db: Arc<anlg_db_core::Db>,
    authorizer: Option<Arc<WebviewSqlAuthorizer>>,
}

impl DbExecutor {
    pub fn new(db: Arc<anlg_db_core::Db>) -> Self {
        Self {
            db,
            authorizer: None,
        }
    }

    pub fn for_webview(db: Arc<anlg_db_core::Db>, authorizer: Arc<WebviewSqlAuthorizer>) -> Self {
        Self {
            db,
            authorizer: Some(authorizer),
        }
    }

    pub fn authorizer(&self) -> Option<&Arc<WebviewSqlAuthorizer>> {
        self.authorizer.as_ref()
    }

    pub async fn execute(
        &self,
        sql: String,
        params: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>> {
        let Some(authorizer) = &self.authorizer else {
            return query::run_query(self.db.pool(), &sql, &params)
                .await
                .map_err(Into::into);
        };
        let mut connection = AuthorizedConnection::acquire(self.db.pool(), authorizer).await?;
        let result = match connection.workspace_guard() {
            None => query::run_query(connection.connection(), &sql, &params)
                .await
                .map_err(Into::into),
            Some(guard) => {
                let read_only = anlg_db_core::is_read_only_sql(&sql);
                let conn = connection.connection();
                let statement = sql.clone();
                guarded(conn, &guard, read_only, &statement, move |conn| {
                    let (sql, params) = (sql.clone(), params.clone());
                    Box::pin(async move { query::run_query(conn, &sql, &params).await })
                })
                .await
            }
        };
        connection.release().await?;
        result
    }

    pub async fn execute_proxy(
        &self,
        sql: String,
        params: Vec<serde_json::Value>,
        method: ProxyQueryMethod,
    ) -> Result<ProxyQueryResult> {
        let Some(authorizer) = &self.authorizer else {
            return query::run_query_proxy(self.db.pool(), &sql, &params, method)
                .await
                .map_err(Into::into);
        };
        let mut connection = AuthorizedConnection::acquire(self.db.pool(), authorizer).await?;
        let result = match connection.workspace_guard() {
            None => query::run_query_proxy(connection.connection(), &sql, &params, method)
                .await
                .map_err(Into::into),
            Some(guard) => {
                let read_only =
                    anlg_db_core::is_read_only_sql(&sql) && method != ProxyQueryMethod::Run;
                let conn = connection.connection();
                let statement = sql.clone();
                guarded(conn, &guard, read_only, &statement, move |conn| {
                    let (sql, params) = (sql.clone(), params.clone());
                    Box::pin(
                        async move { query::run_query_proxy(conn, &sql, &params, method).await },
                    )
                })
                .await
            }
        };
        connection.release().await?;
        result
    }

    pub async fn execute_transaction(
        &self,
        statements: Vec<TransactionStatement>,
    ) -> Result<Vec<u64>> {
        let Some(authorizer) = &self.authorizer else {
            let mut connection = self.db.pool().acquire().await?;
            return query::run_transaction(&mut connection, statements).await;
        };
        let mut connection = AuthorizedConnection::acquire(self.db.pool(), authorizer).await?;
        let guard = connection.workspace_guard();
        let result =
            query::run_transaction_guarded(connection.connection(), statements, guard.as_ref())
                .await;
        connection.release().await?;
        result
    }
}

/// Runs one webview statement under the C1 workspace guard: in a
/// transaction Rust opens, after the census, with the post-write census
/// before commit (see [`WorkspaceGuard`]).
async fn guarded<T>(
    connection: &mut SqliteConnection,
    guard: &WorkspaceGuard,
    read_only: bool,
    sql: &str,
    run: impl for<'c> Fn(
        &'c mut SqliteConnection,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = std::result::Result<T, sqlx::Error>> + Send + 'c>,
    >,
) -> Result<T> {
    guard.allow_transaction_control(true);
    let begun = connection
        .begin_with(if read_only {
            "BEGIN"
        } else {
            "BEGIN IMMEDIATE"
        })
        .await;
    guard.allow_transaction_control(false);
    let mut transaction = begun?;
    let result = async {
        let before = guard.changes(&mut transaction).await?;
        let value = if read_only {
            // Census only the tables the read names; if it reaches another
            // content table, a full census in the same snapshot, then retry.
            guard.check_read(&mut transaction, sql).await;
            match run(&mut transaction).await {
                Err(error) if is_denied(&error) => {
                    guard.check(&mut transaction).await;
                    run(&mut transaction).await?
                }
                other => other?,
            }
        } else {
            guard.check(&mut transaction).await;
            run(&mut transaction).await?
        };
        guard.check_writes(&mut transaction, before).await?;
        Ok::<_, sqlx::Error>(value)
    }
    .await;
    guard.allow_transaction_control(true);
    let finished = match &result {
        Ok(_) => transaction.commit().await,
        Err(_) => transaction.rollback().await,
    };
    guard.allow_transaction_control(false);
    finished?;
    result.map_err(Into::into)
}

fn is_denied(error: &sqlx::Error) -> bool {
    let message = error.to_string();
    message.contains("not authorized") || message.contains("prohibited")
}
