import {
  type ChatMessageRecord,
  type ChatMessageSqlRow,
  type PersistedChatMessage,
  rowToPersistedChatMessage,
} from "./persisted-messages";

import type { ChatScope } from "~/chat/types";
import { executeTransaction, liveQueryClient, useLiveQuery } from "~/db";
import { enqueueDatabaseWrite } from "~/db/write-queue";

type ChatGroupSqlRow = {
  id: string;
  owner_user_id: string;
  title: string;
  created_at: string;
  updated_at: string;
};

export type ChatGroupRecord = {
  id: string;
  ownerUserId: string;
  title: string;
  createdAt: string;
  updatedAt: string;
};

const EMPTY_CHAT_GROUPS: ChatGroupRecord[] = [];
const EMPTY_CHAT_MESSAGES: PersistedChatMessage[] = [];

export function useRecentChatGroups(
  chatScope: ChatScope,
  limit = 5,
  askThreadKey?: string,
): ChatGroupRecord[] {
  return useChatGroupsQuery(chatScope, limit, askThreadKey);
}

export function useChatGroups(chatScope: ChatScope): ChatGroupRecord[] {
  return useChatGroupsQuery(chatScope);
}

/**
 * General chats from before scoped Ask threads (no user message carries an
 * askScope). Listed under "Previous chats"; opened read-only in General and
 * never part of a keyed thread's model context.
 */
export function usePreviousChatGroups(
  enabled: boolean,
  limit = 5,
): ChatGroupRecord[] {
  const { data = EMPTY_CHAT_GROUPS } = useLiveQuery<
    ChatGroupSqlRow,
    ChatGroupRecord[]
  >({
    sql: `
      SELECT g.id, g.owner_user_id, g.title, g.created_at, g.updated_at
      FROM chat_groups AS g
      WHERE g.deleted_at IS NULL
        AND ${chatGroupScopePredicate("general")}
        AND NOT ${ANY_ASK_SCOPE_PREDICATE}
      ORDER BY g.created_at DESC, g.id DESC
      LIMIT ?
    `,
    params: [limit],
    enabled,
    mapRows: (rows) => rows.map(mapChatGroupRow),
  });

  return enabled ? data : EMPTY_CHAT_GROUPS;
}

function useChatGroupsQuery(
  chatScope: ChatScope,
  limit?: number,
  askThreadKey?: string,
): ChatGroupRecord[] {
  const askThread = askThreadKey ? askThreadSqlParams(askThreadKey) : null;
  const { data = EMPTY_CHAT_GROUPS } = useLiveQuery<
    ChatGroupSqlRow,
    ChatGroupRecord[]
  >({
    sql: `
      SELECT g.id, g.owner_user_id, g.title, g.created_at, g.updated_at
      FROM chat_groups AS g
      WHERE g.deleted_at IS NULL
        AND ${chatGroupScopePredicate(chatScope)}
        ${askThread ? `AND ${ASK_THREAD_PREDICATE}` : ""}
      ORDER BY g.created_at DESC, g.id DESC
      ${limit === undefined ? "" : "LIMIT ?"}
    `,
    params: [
      ...(askThread ? [askThread.scope, askThread.meetingId] : []),
      ...(limit === undefined ? [] : [limit]),
    ],
    mapRows: (rows) => rows.map(mapChatGroupRow),
  });

  return data;
}

export function useChatGroup(
  chatGroupId: string | null | undefined,
  chatScope: ChatScope,
): ChatGroupRecord | null {
  const { data = null } = useLiveQuery<ChatGroupSqlRow, ChatGroupRecord | null>(
    {
      sql: `
      SELECT g.id, g.owner_user_id, g.title, g.created_at, g.updated_at
      FROM chat_groups AS g
      WHERE g.id = ?
        AND g.deleted_at IS NULL
        AND ${chatGroupScopePredicate(chatScope)}
      LIMIT 1
    `,
      params: [chatGroupId ?? ""],
      enabled: Boolean(chatGroupId),
      mapRows: (rows) => {
        const row = rows.find(({ id }) => id === chatGroupId);
        return row ? mapChatGroupRow(row) : null;
      },
    },
  );

  return chatGroupId ? data : null;
}

export function usePersistedChatMessages(
  chatGroupId: string | null | undefined,
): PersistedChatMessage[] {
  const { data = EMPTY_CHAT_MESSAGES } = useLiveQuery<
    ChatMessageSqlRow,
    PersistedChatMessage[]
  >({
    sql: `
      SELECT
        id,
        owner_user_id,
        created_at,
        chat_group_id,
        role,
        content,
        metadata_json,
        parts_json,
        status
      FROM chat_messages
      WHERE chat_group_id = ? AND deleted_at IS NULL
      ORDER BY created_at, id
    `,
    params: [chatGroupId ?? ""],
    enabled: Boolean(chatGroupId),
    mapRows: (rows) =>
      rows
        .filter((row) => row.chat_group_id === chatGroupId)
        .map(rowToPersistedChatMessage),
  });

  return chatGroupId ? data : EMPTY_CHAT_MESSAGES;
}

export function createChatGroupWithMessage({
  groupId,
  ownerUserId,
  title,
  createdAt,
  message,
}: {
  groupId: string;
  ownerUserId: string;
  title: string;
  createdAt: string;
  message: ChatMessageRecord;
}): Promise<void> {
  if (message.chatGroupId !== groupId) {
    return Promise.reject(
      new Error("chat message group does not match the group being created"),
    );
  }

  return enqueueDatabaseWrite(chatWriteKey(groupId), async () => {
    const now = new Date().toISOString();
    await executeTransaction([
      {
        sql: `
          INSERT INTO chat_groups (
            id, workspace_id, owner_user_id, title, created_at, updated_at,
            deleted_at
          )
          VALUES (?, '', ?, ?, ?, ?, NULL)
          ON CONFLICT(id) DO UPDATE SET
            owner_user_id = excluded.owner_user_id,
            title = excluded.title,
            updated_at = excluded.updated_at,
            deleted_at = NULL
        `,
        params: [groupId, ownerUserId, title, createdAt, now],
      },
      buildUpsertChatMessageStatement(message, now),
    ]);
  });
}

export function upsertChatMessage(message: ChatMessageRecord): Promise<void> {
  return enqueueDatabaseWrite(chatWriteKey(message.chatGroupId), async () => {
    await executeTransaction([
      buildUpsertChatMessageStatement(message, new Date().toISOString()),
    ]);
  });
}

export function replaceChatMessage({
  message,
  previousMessageId,
}: {
  message: ChatMessageRecord;
  previousMessageId: string;
}): Promise<void> {
  return enqueueDatabaseWrite(chatWriteKey(message.chatGroupId), async () => {
    const now = new Date().toISOString();
    const statements = [buildUpsertChatMessageStatement(message, now)];
    if (previousMessageId !== message.id) {
      statements.push(
        buildDeleteChatMessageStatement(
          message.chatGroupId,
          previousMessageId,
          now,
        ),
      );
    }
    await executeTransaction(statements);
  });
}

export function setChatGroupTitleIfCurrent({
  groupId,
  expectedTitle,
  title,
}: {
  groupId: string;
  expectedTitle: string;
  title: string;
}): Promise<void> {
  return enqueueDatabaseWrite(chatWriteKey(groupId), async () => {
    const now = new Date().toISOString();
    await executeTransaction([
      {
        sql: `
          UPDATE chat_groups
          SET title = ?, updated_at = ?
          WHERE id = ? AND title = ? AND deleted_at IS NULL
        `,
        params: [title, now, groupId, expectedTitle],
      },
    ]);
  });
}

export async function getChatMessageGroupId(
  messageId: string,
): Promise<string | null> {
  const rows = await liveQueryClient.execute<{ chat_group_id: string }>(
    `
      SELECT chat_group_id
      FROM chat_messages
      WHERE id = ? AND deleted_at IS NULL
      LIMIT 1
    `,
    [messageId],
  );

  return rows[0]?.chat_group_id || null;
}

export function deleteChatGroup(chatGroupId: string): Promise<void> {
  return enqueueDatabaseWrite(chatWriteKey(chatGroupId), async () => {
    const now = new Date().toISOString();
    await executeTransaction([
      {
        sql: `
          UPDATE chat_groups
          SET deleted_at = ?, updated_at = ?
          WHERE id = ? AND deleted_at IS NULL
        `,
        params: [now, now, chatGroupId],
      },
      {
        sql: `
          UPDATE chat_messages
          SET deleted_at = ?, updated_at = ?
          WHERE chat_group_id = ? AND deleted_at IS NULL
        `,
        params: [now, now, chatGroupId],
      },
    ]);
  });
}

export function deleteChatMessage(
  chatGroupId: string,
  messageId: string,
): Promise<void> {
  return enqueueDatabaseWrite(chatWriteKey(chatGroupId), async () => {
    const now = new Date().toISOString();
    await executeTransaction([
      buildDeleteChatMessageStatement(chatGroupId, messageId, now),
    ]);
  });
}

export function deleteChatMessagesExcept(
  chatGroupId: string,
  retainedMessageIds: string[],
): Promise<void> {
  return enqueueDatabaseWrite(chatWriteKey(chatGroupId), async () => {
    const now = new Date().toISOString();
    await executeTransaction([
      {
        sql: `
          UPDATE chat_messages
          SET deleted_at = ?, updated_at = ?
          WHERE chat_group_id = ?
            AND deleted_at IS NULL
            AND id NOT IN (SELECT value FROM json_each(?))
        `,
        params: [now, now, chatGroupId, JSON.stringify(retainedMessageIds)],
      },
    ]);
  });
}

function mapChatGroupRow(row: ChatGroupSqlRow): ChatGroupRecord {
  return {
    id: row.id,
    ownerUserId: row.owner_user_id,
    title: row.title,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

// A group belongs to the Ask thread its first user message was sent in.
// Groups from before scoped threads have no askScope and never match.
const ASK_THREAD_PREDICATE = `
  EXISTS (
    SELECT 1
    FROM chat_messages AS am
    WHERE am.chat_group_id = g.id
      AND am.deleted_at IS NULL
      AND am.role = 'user'
      AND json_valid(am.metadata_json)
      AND json_extract(am.metadata_json, '$.askScope') = ?
      AND COALESCE(json_extract(am.metadata_json, '$.askMeetingId'), '') = ?
  )
`;

const ANY_ASK_SCOPE_PREDICATE = `
  EXISTS (
    SELECT 1
    FROM chat_messages AS am
    WHERE am.chat_group_id = g.id
      AND am.deleted_at IS NULL
      AND am.role = 'user'
      AND json_valid(am.metadata_json)
      AND json_extract(am.metadata_json, '$.askScope') IS NOT NULL
  )
`;

function askThreadSqlParams(askThreadKey: string) {
  if (askThreadKey.startsWith("meeting:")) {
    return {
      scope: "meeting",
      meetingId: askThreadKey.slice("meeting:".length),
    };
  }
  return { scope: askThreadKey, meetingId: "" };
}

function chatGroupScopePredicate(chatScope: ChatScope) {
  const automationsScopeExists = `
    EXISTS (
      SELECT 1
      FROM chat_messages AS m
      WHERE m.chat_group_id = g.id
        AND m.deleted_at IS NULL
        AND CASE
          WHEN json_valid(m.metadata_json)
            THEN json_extract(m.metadata_json, '$.chatScope')
          ELSE NULL
        END = 'automations'
    )
  `;

  return chatScope === "automations"
    ? automationsScopeExists
    : `NOT ${automationsScopeExists}`;
}

function buildUpsertChatMessageStatement(
  message: ChatMessageRecord,
  updatedAt: string,
) {
  return {
    sql: `
      INSERT INTO chat_messages (
        id, workspace_id, chat_group_id, owner_user_id, role, content,
        metadata_json, parts_json, status, created_at, updated_at, deleted_at
      )
      VALUES (?, '', ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
      ON CONFLICT(id) DO UPDATE SET
        chat_group_id = excluded.chat_group_id,
        owner_user_id = excluded.owner_user_id,
        role = excluded.role,
        content = excluded.content,
        metadata_json = excluded.metadata_json,
        parts_json = excluded.parts_json,
        status = excluded.status,
        updated_at = excluded.updated_at,
        deleted_at = NULL
    `,
    params: [
      message.id,
      message.chatGroupId,
      message.ownerUserId,
      message.role,
      message.content,
      message.metadataJson,
      message.partsJson,
      message.status,
      message.createdAt,
      updatedAt,
    ],
  };
}

function buildDeleteChatMessageStatement(
  chatGroupId: string,
  messageId: string,
  updatedAt: string,
) {
  return {
    sql: `
      UPDATE chat_messages
      SET deleted_at = ?, updated_at = ?
      WHERE id = ? AND chat_group_id = ? AND deleted_at IS NULL
    `,
    params: [updatedAt, updatedAt, messageId, chatGroupId],
  };
}

function chatWriteKey(chatGroupId: string) {
  return `chat:${chatGroupId}`;
}
