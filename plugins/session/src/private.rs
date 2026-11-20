//! ASPC C6.4 / D1.4: marking a meeting Private, written in Rust so every
//! change is audit-logged (`meeting.private_changed`).

use sqlx::SqlitePool;

/// Sets `sessions.private`. Returns true when the flag changed (a deleted
/// meeting or an unchanged flag is left alone and not logged).
pub async fn set_session_private(
    pool: &SqlitePool,
    session_id: &str,
    private: bool,
) -> Result<bool, String> {
    let result = sqlx::query(
        "UPDATE sessions
         SET private = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?2 AND deleted_at IS NULL AND private IS NOT ?1",
    )
    .bind(i64::from(private))
    .bind(session_id)
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(result.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sets_and_clears_the_flag_once() {
        let db = anlg_db_core::Db::connect_memory_plain().await.unwrap();
        anlg_db_app::prepare_schema(&db).await.unwrap();
        sqlx::query("INSERT INTO sessions (id) VALUES ('a'), ('gone')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("UPDATE sessions SET deleted_at = '2026-11-19T00:00:00Z' WHERE id = 'gone'")
            .execute(db.pool())
            .await
            .unwrap();
        let private = |id: &'static str| {
            let pool = db.pool().clone();
            async move {
                sqlx::query_scalar::<_, i64>("SELECT private FROM sessions WHERE id = ?")
                    .bind(id)
                    .fetch_one(&pool)
                    .await
                    .unwrap()
            }
        };

        assert!(set_session_private(db.pool(), "a", true).await.unwrap());
        assert_eq!(private("a").await, 1);
        assert!(
            !set_session_private(db.pool(), "a", true).await.unwrap(),
            "unchanged"
        );
        assert!(set_session_private(db.pool(), "a", false).await.unwrap());
        assert_eq!(private("a").await, 0);
        assert!(!set_session_private(db.pool(), "gone", true).await.unwrap());
        assert_eq!(private("gone").await, 0);
        assert!(
            !set_session_private(db.pool(), "missing", true)
                .await
                .unwrap()
        );
    }
}
