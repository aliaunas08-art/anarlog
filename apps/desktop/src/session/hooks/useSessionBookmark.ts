import { useLingui } from "@lingui/react/macro";
import { useCallback } from "react";

import { toast } from "@anlg/ui/components/ui/toast";

import type { NoteInputHandle } from "../components/note-input";

import { appendParagraphToStoredMemo, formatElapsed } from "~/session/bookmark";

/**
 * Bookmarks the current recording moment as a "🔖 Bookmark · m:ss" line at the
 * end of the memo: through the live editor when the memo is the mounted view,
 * otherwise straight into the stored memo.
 */
export function useSessionBookmark(
  sessionId: string,
  noteInputRef: React.RefObject<NoteInputHandle | null>,
) {
  const { t } = useLingui();

  return useCallback(
    async (elapsedSeconds: number) => {
      const time = formatElapsed(elapsedSeconds);
      const line = `🔖 ${t`Bookmark`} · ${time}`;

      try {
        const inserted =
          noteInputRef.current?.appendMemoParagraph(line) ?? false;
        if (!inserted) {
          await appendParagraphToStoredMemo(sessionId, line);
        }
        toast.success(t`Bookmarked ${time}`, {
          id: `session-bookmark-${sessionId}`,
          duration: 2000,
        });
      } catch (error) {
        console.error("[session] failed to add bookmark", error);
        toast.error(t`Couldn't add the bookmark. Open Notes and try again.`);
      }
    },
    [noteInputRef, sessionId, t],
  );
}
