import { liveQueryClient } from "~/db";
import { normalizeKeywordList } from "~/stt/keywords";

const MAX_TRANSCRIPTION_HINTS = 50;

type KeywordSnapshotSqlRow = {
  raw_md: string;
  title: string;
  event_json: string;
  participant_names_json: string;
  event_participants_json: string;
};

export async function getSessionKeywords({
  sessionId,
  dictionaryTerms,
}: {
  sessionId: string;
  dictionaryTerms: string[];
}): Promise<string[]> {
  const [snapshot] = await liveQueryClient.execute<KeywordSnapshotSqlRow>(
    `
      SELECT
        COALESCE(note.body, '') AS raw_md,
        session.title,
        session.event_json,
        COALESCE((
          SELECT json_group_array(name)
          FROM (
            SELECT COALESCE(NULLIF(human.name, ''), participant.display_name) AS name
            FROM session_participants AS participant
            LEFT JOIN humans AS human
              ON human.id = participant.human_id
              AND human.deleted_at IS NULL
            WHERE participant.session_id = session.id
              AND participant.source <> 'excluded'
              AND participant.deleted_at IS NULL
              AND COALESCE(NULLIF(human.name, ''), participant.display_name) <> ''
            ORDER BY name, participant.id
          )
        ), '[]') AS participant_names_json,
        COALESCE((
          SELECT event.participants_json
          FROM events AS event
          WHERE event.deleted_at IS NULL
            AND (
              event.id = session.event_id
              OR (
                event.tracking_id_event = CASE
                  WHEN json_valid(session.event_json)
                  THEN json_extract(session.event_json, '$.tracking_id')
                  ELSE ''
                END
                AND event.calendar_id = CASE
                  WHEN json_valid(session.event_json)
                  THEN json_extract(session.event_json, '$.calendar_id')
                  ELSE ''
                END
              )
            )
          ORDER BY event.started_at, event.id
          LIMIT 1
        ), '[]') AS event_participants_json
      FROM sessions AS session
      LEFT JOIN session_documents AS note
        ON note.id = session.id
        AND note.kind = 'note'
        AND note.deleted_at IS NULL
      WHERE session.id = ? AND session.deleted_at IS NULL
      LIMIT 1
    `,
    [sessionId],
  );

  return buildKeywords({
    rawMd: snapshot?.raw_md,
    title: snapshot?.title,
    eventJson: snapshot?.event_json,
    sessionParticipantTerms: parseStringList(snapshot?.participant_names_json),
    eventParticipantTerms: parseEventParticipantNames(
      snapshot?.event_participants_json,
    ),
    dictionaryTerms,
  });
}

export function buildKeywords({
  rawMd,
  title,
  eventJson,
  sessionParticipantTerms = [],
  eventParticipantTerms = [],
  dictionaryTerms,
}: {
  rawMd: unknown;
  title: unknown;
  eventJson: unknown;
  sessionParticipantTerms?: string[];
  eventParticipantTerms?: string[];
  dictionaryTerms: string[];
}) {
  // Hints are participants, dictionary terms and #hashtags from the notes,
  // title and event. Automatic keyword extraction (retext-keywords) was
  // removed with its LGPL part-of-speech tagger, `pos` (ASPC D5).
  const hashtags = extractHashtagKeywords(
    buildKeywordSourceText({ rawMd, title, eventJson }),
  );

  return normalizeKeywordList([
    ...sessionParticipantTerms,
    ...eventParticipantTerms,
    ...dictionaryTerms,
    ...hashtags,
  ]).slice(0, MAX_TRANSCRIPTION_HINTS);
}

export function buildKeywordSourceText({
  rawMd,
  title,
  eventJson,
}: {
  rawMd: unknown;
  title: unknown;
  eventJson: unknown;
}): string {
  return [
    stringValue(rawMd),
    stringValue(title),
    ...eventKeywordFields(eventJson),
  ]
    .filter((value) => value.length > 0)
    .join("\n");
}

/** #hashtags outside code, two characters or longer. */
export const extractHashtagKeywords = (markdown: string): string[] =>
  extractHashtags(removeCodeBlocks(markdown)).filter(
    (keyword) => keyword.length >= 2,
  );

const stringValue = (value: unknown): string =>
  typeof value === "string" ? value.trim() : "";

const eventKeywordFields = (eventJson: unknown): string[] => {
  if (typeof eventJson !== "string" || !eventJson) {
    return [];
  }

  try {
    const event = JSON.parse(eventJson);
    return [event?.title, event?.description, event?.location].flatMap(
      (value) => {
        const text = stringValue(value);
        return text ? [text] : [];
      },
    );
  } catch {
    return [];
  }
};

const parseStringList = (value: unknown): string[] => {
  if (typeof value !== "string" || !value) return [];
  try {
    const parsed = JSON.parse(value);
    return Array.isArray(parsed)
      ? parsed.filter((entry): entry is string => typeof entry === "string")
      : [];
  } catch {
    return [];
  }
};

const parseEventParticipantNames = (participantsJson: unknown): string[] => {
  if (typeof participantsJson !== "string" || !participantsJson) {
    return [];
  }

  try {
    const participants = JSON.parse(participantsJson);
    if (!Array.isArray(participants)) {
      return [];
    }

    return participants.flatMap((participant) => {
      if (participant?.is_current_user === true) {
        return [];
      }

      const name = stringValue(participant?.name);
      return name ? [name] : [];
    });
  } catch {
    return [];
  }
};

const removeCodeBlocks = (text: string): string =>
  text.replace(/```[\s\S]*?```/g, "").replace(/`[^`]+`/g, "");

const extractHashtags = (text: string): string[] =>
  Array.from(text.matchAll(/#([\p{L}\p{N}_]+)/gu), (match) => match[1]).filter(
    Boolean,
  );
