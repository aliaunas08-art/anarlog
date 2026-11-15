import { useLingui } from "@lingui/react/macro";
import { useMemo } from "react";

import {
  copyTextToClipboard,
  getStoredNoteMarkdown,
} from "./note-input/header-shared";
import { useShowSessionShare } from "./outer-header";

import { queueChatPrompt } from "~/chat/components/input/hooks";
import { useChatContext } from "~/chat/state/chat-context";
import { useShell } from "~/contexts/shell";
import { useEnhancedNote } from "~/session/queries";

const CAPSULE =
  "cf-capsule cf-motion rounded-pill transition-[filter,background-color] hover:brightness-[0.97] focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-none dark:hover:brightness-110";

/**
 * Board capsules under the Minutes: each one reuses an existing flow (the
 * chat's follow-up prompt, the summary's Copy, the header Share popover and
 * the Transcript view). A capsule without a working flow is left out.
 */
export function SessionActionCapsules({
  sessionId,
  enhancedNoteId,
  onOpenTranscript,
}: {
  sessionId: string;
  enhancedNoteId: string;
  onOpenTranscript?: () => void;
}) {
  const { t } = useLingui();
  const { chat } = useShell();
  const enhancedNote = useEnhancedNote(enhancedNoteId);
  const markdown = useMemo(
    () => getStoredNoteMarkdown(enhancedNote?.content),
    [enhancedNote?.content],
  );
  const showShare = useShowSessionShare(sessionId);

  // Nothing to act on until the summary has content.
  if (!markdown) return null;

  const draftFollowUp = () => {
    // The draft needs this meeting's transcript: use its "This meeting" thread.
    const threadSessionId = useChatContext
      .getState()
      .prepareAskThread("meeting", sessionId);
    queueChatPrompt(
      threadSessionId,
      t`Draft a follow-up email to the participants`,
    );
    chat.sendEvent({ type: "OPEN" });
  };
  const copyMarkdown = () => {
    void copyTextToClipboard(markdown, {
      success: t`Copied as Markdown`,
      error: t`Couldn't copy the summary`,
    });
  };
  const shareLink = () => {
    // The header Share button owns the share popover and its whole flow.
    document
      .querySelector<HTMLElement>(
        `[data-session-share-trigger="${CSS.escape(sessionId)}"]`,
      )
      ?.click();
  };

  return (
    <div
      role="group"
      aria-label={t`Meeting actions`}
      data-session-action-capsules
      className="flex flex-wrap gap-2 pt-[26px]"
    >
      <button type="button" className={CAPSULE} onClick={draftFollowUp}>
        {t`Draft follow-up email`}
      </button>
      <button type="button" className={CAPSULE} onClick={copyMarkdown}>
        {t`Copy as Markdown`}
      </button>
      {showShare ? (
        <button
          type="button"
          className={CAPSULE}
          aria-haspopup="dialog"
          onClick={shareLink}
        >
          {t`Share link`}
        </button>
      ) : null}
      {onOpenTranscript ? (
        <button type="button" className={CAPSULE} onClick={onOpenTranscript}>
          {t`Open transcript`}
        </button>
      ) : null}
    </div>
  );
}
