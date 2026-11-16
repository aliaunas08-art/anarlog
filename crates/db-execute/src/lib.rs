#![forbid(unsafe_code)]

mod error;
mod query;

use std::sync::Arc;

use anlg_db_core::{AuthorizedConnection, WebviewSqlAuthorizer};

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
        let result = query::run_query(connection.connection(), &sql, &params).await;
        connection.release().await?;
        result.map_err(Into::into)
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
        let result = query::run_query_proxy(connection.connection(), &sql, &params, method).await;
        connection.release().await?;
        result.map_err(Into::into)
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
        let result = query::run_transaction(connection.connection(), statements).await;
        connection.release().await?;
        result
    }
}
