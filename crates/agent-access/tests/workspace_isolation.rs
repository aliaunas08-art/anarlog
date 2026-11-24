//! ASPC C1.4: two workspaces hold marked content. Every list, read, export,
//! transcript, history, folder and proposal path called for workspace 1
//! returns nothing from workspace 2. The CLI, MCP, local-api and the `db:*`
//! meeting commands all go through these functions with the local scope.

use agent_access::{
    Error, GetMeetingInput, GetMeetingTranscriptInput, GetProposalInput,
    GetRecurringMeetingHistoryInput, ListFoldersInput, ListMeetingsInput, ListProposalsInput,
    WorkspaceScope,
};

const W1: &str = "workspace-one";
const W2: &str = "workspace-two";
const MARK2: &str = "W2-SECRET-MARKER";

async fn db() -> anlg_db_core::Db {
    let db = anlg_db_core::Db::connect_memory_plain().await.unwrap();
    anlg_db_app::prepare_schema(&db).await.unwrap();
    sqlx::query("UPDATE app_settings SET value_json = ? WHERE id = 'cloudsync_workspace_binding'")
        .bind(format!(
            "{{\"workspace_id\":\"{W1}\",\"account_user_id\":null}}"
        ))
        .execute(db.pool())
        .await
        .unwrap();
    for (ws, mark) in [(W1, "W1-MARKER"), (W2, MARK2)] {
        let session = format!("{ws}-meeting");
        let human = format!("{ws}-human");
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "INSERT INTO sessions (id, workspace_id, owner_user_id, title, started_at, series_id, folder_path)
               VALUES ('{session}', '{ws}', '{ws}', '{mark} meeting', '2026-07-01', 'shared-series', '{mark}-folder');
             INSERT INTO session_documents (id, workspace_id, session_id, kind, body_format, body, title)
               VALUES ('{session}', '{ws}', '{session}', 'note', 'markdown', '{mark} note', 'Notes'),
                      ('{session}-summary', '{ws}', '{session}', 'summary', 'markdown', '{mark} summary', 'Summary');
             INSERT INTO humans (id, workspace_id, owner_user_id, name)
               VALUES ('{human}', '{ws}', '{ws}', '{mark} person');
             INSERT INTO session_participants (id, workspace_id, owner_user_id, session_id, human_id, display_name)
               VALUES ('{session}-p', '{ws}', '{ws}', '{session}', '{human}', '{mark} attendee');
             INSERT INTO action_items (id, workspace_id, session_id, status, text)
               VALUES ('{session}-a', '{ws}', '{session}', 'open', '{mark} action');
             INSERT INTO transcripts (id, workspace_id, owner_user_id, session_id, started_at_ms, words_json)
               VALUES ('{session}-t', '{ws}', '{ws}', '{session}', 0, '[{{\"text\":\"{mark}\"}}]');
             INSERT INTO folders (id, workspace_id, path) VALUES ('{ws}-folder', '{ws}', '{mark}-empty-folder');
             INSERT INTO session_proposals (id, workspace_id, session_id, kind, target_id, current_markdown, proposed_markdown, status, source)
               VALUES ('{ws}-proposal', '{ws}', '{session}', 'summary_replace', '{session}-summary', 'a', '{mark} proposal', 'pending', 'cli');"
        )))
        .execute(db.pool())
        .await
        .unwrap();
    }
    // A workspace-2 human that shares an id with a workspace-1 participant's
    // human must not lend its name either.
    sqlx::query("INSERT INTO humans (id, workspace_id, name) VALUES ('shared-human', ?, ?)")
        .bind(W2)
        .bind(format!("{MARK2} shared"))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE session_participants SET human_id = 'shared-human' WHERE id = ?")
        .bind(format!("{W1}-meeting-p"))
        .execute(db.pool())
        .await
        .unwrap();
    db
}

fn has_mark<T: serde::Serialize>(value: &T) -> bool {
    serde_json::to_string(value).unwrap().contains(MARK2)
}

fn not_found<T: std::fmt::Debug>(result: agent_access::Result<T>) -> bool {
    matches!(result, Err(Error::NotFound(_)))
}

#[tokio::test]
async fn workspace_one_never_sees_workspace_two() {
    let db = db().await;
    let pool = db.pool();
    let scope = agent_access::local_scope(pool).await.unwrap();
    assert_eq!(scope, WorkspaceScope::local(W1).unwrap());
    let w2_meeting = format!("{W2}-meeting");
    let w1_meeting = format!("{W1}-meeting");

    let listed = agent_access::list_meetings(pool, &scope, ListMeetingsInput::default())
        .await
        .unwrap();
    assert_eq!(listed.meetings.len(), 1);
    assert!(!has_mark(&listed));
    let searched = agent_access::list_meetings(
        pool,
        &scope,
        ListMeetingsInput {
            query: Some("meeting".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!has_mark(&searched));

    let folders = agent_access::list_folders(pool, &scope, ListFoldersInput::default())
        .await
        .unwrap();
    assert!(!has_mark(&folders), "{folders:?}");

    let meeting = agent_access::get_meeting(
        pool,
        &scope,
        GetMeetingInput {
            meeting_id: w1_meeting.clone(),
        },
    )
    .await
    .unwrap();
    assert!(!has_mark(&meeting), "{meeting:?}");
    assert!(not_found(
        agent_access::get_meeting(
            pool,
            &scope,
            GetMeetingInput {
                meeting_id: w2_meeting.clone()
            }
        )
        .await
    ));
    assert!(not_found(
        agent_access::get_meeting_transcript(
            pool,
            &scope,
            GetMeetingTranscriptInput {
                meeting_id: w2_meeting.clone(),
                offset: None,
                limit: None,
            }
        )
        .await
    ));
    let transcript = agent_access::get_meeting_transcript(
        pool,
        &scope,
        GetMeetingTranscriptInput {
            meeting_id: w1_meeting.clone(),
            offset: None,
            limit: None,
        },
    )
    .await
    .unwrap();
    assert!(!has_mark(&transcript));

    // Both meetings share a series id; history stays in workspace 1.
    let history = agent_access::get_recurring_meeting_history(
        pool,
        &scope,
        GetRecurringMeetingHistoryInput {
            meeting_id: w1_meeting.clone(),
            limit: None,
            offset: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(history.meetings.len(), 1);
    assert!(!has_mark(&history));

    // Export (local-api markdown, cloud snapshot, Drive, webhooks, CLI).
    let export = agent_access::get_meeting_export(pool, &scope, w1_meeting.clone())
        .await
        .unwrap();
    assert!(!has_mark(&export), "{export:?}");
    assert!(!export.to_markdown().contains(MARK2));
    assert!(not_found(
        agent_access::get_meeting_export(pool, &scope, w2_meeting.clone()).await
    ));
    let local_export = agent_access::get_local_meeting_export(pool, &scope, w1_meeting.clone())
        .await
        .unwrap();
    assert!(!has_mark(&local_export));
    assert!(not_found(
        agent_access::get_local_meeting_export(pool, &scope, w2_meeting.clone()).await
    ));

    // Proposals.
    let proposals = agent_access::list_proposals(
        pool,
        &scope,
        ListProposalsInput {
            meeting_id: None,
            status: Some("all".into()),
            limit: None,
            offset: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(proposals.proposals.len(), 1);
    assert!(!has_mark(&proposals));
    assert!(not_found(
        agent_access::get_proposal(
            pool,
            &scope,
            GetProposalInput {
                proposal_id: format!("{W2}-proposal")
            }
        )
        .await
    ));

    // The same calls for workspace 2 do find its rows (the filter is the
    // scope, not a missing row).
    let other = WorkspaceScope::new(W2).unwrap();
    let w2 = agent_access::get_meeting_export(pool, &other, w2_meeting)
        .await
        .unwrap();
    assert!(has_mark(&w2));
}

/// Without a local workspace the scope cannot be built: callers fail closed.
#[tokio::test]
async fn no_local_workspace_means_no_scope() {
    let db = db().await;
    sqlx::query("DELETE FROM app_settings WHERE id = 'cloudsync_workspace_binding'")
        .execute(db.pool())
        .await
        .unwrap();
    assert!(agent_access::local_scope(db.pool()).await.is_err());
}
