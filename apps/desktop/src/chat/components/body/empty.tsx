import { t } from "@lingui/core/macro";
import { useCallback } from "react";

import {
  ArrowsInSimple,
  ArrowsOutSimple,
  ClockCounterClockwise,
  Envelope,
  Lightbulb,
  ListChecks,
  MagnifyingGlass,
  Sparkle,
} from "@anlg/ui/components/icons";
import { cn } from "@anlg/utils";

import type { ContextRef } from "~/chat/context/entities";
import type { AskExplicitIntent } from "~/chat/guardrails/classifier";
import type { AskScope } from "~/chat/guardrails/scope";
import { useChatAppearance } from "~/chat/hooks/use-chat-appearance";
import { APP_NAME } from "~/shared/app-name";
import { useTabs } from "~/store/zustand/tabs";

type Suggestion = {
  label: string;
  icon: typeof Envelope;
  prompt: string;
  /** Summary chips are explicit edit requests (edit tools stay available). */
  askIntent?: AskExplicitIntent;
};

export function ChatBodyEmpty({
  isModelConfigured = true,
  hasContext = false,
  isLiveMeeting = false,
  hasSummary = false,
  askScope,
  onSendMessage,
}: {
  isModelConfigured?: boolean;
  hasContext?: boolean;
  isLiveMeeting?: boolean;
  hasSummary?: boolean;
  askScope?: AskScope;
  onSendMessage?: (
    content: string,
    parts: Array<{ type: "text"; text: string }>,
    contextRefs?: ContextRef[],
    options?: { askIntent?: AskExplicitIntent },
  ) => void;
}) {
  const { isDarkAppearance } = useChatAppearance();
  const openNew = useTabs((state) => state.openNew);
  const liveMeetingSuggestions: Suggestion[] = [
    {
      label: t`Recap the last 5 minutes.`,
      icon: ClockCounterClockwise,
      prompt: t`Recap what was discussed in the last 5 minutes of this meeting. Use the transcript timestamps, where the latest one is now.`,
    },
    {
      label: t`Suggest what to ask next.`,
      icon: Lightbulb,
      prompt: t`Based on the conversation so far, suggest a few questions I could ask next.`,
    },
    {
      label: t`List action items so far.`,
      icon: ListChecks,
      prompt: t`What action items have come up so far in this meeting?`,
    },
  ];
  const summarySuggestions: Suggestion[] = hasSummary
    ? [
        {
          label: t`Make the summary shorter.`,
          icon: ArrowsInSimple,
          askIntent: "edit_summary",
          prompt: t`Make the current summary noticeably shorter than it is now. Keep every decision, owner, deadline, and open question; cut repetition and secondary detail.`,
        },
        {
          label: t`Make the summary longer.`,
          icon: ArrowsOutSimple,
          askIntent: "edit_summary",
          prompt: t`Make the current summary more detailed than it is now. Add supporting context, rationale, and examples from the transcript without inventing anything.`,
        },
      ]
    : [];
  const meetingSuggestions: Suggestion[] = [
    ...summarySuggestions,
    {
      label: t`List action items.`,
      icon: ListChecks,
      prompt: hasContext
        ? t`What are my action items from this meeting?`
        : t`What are my outstanding action items across my meetings?`,
    },
    {
      label: t`Draft follow-up email.`,
      icon: Envelope,
      prompt: hasContext
        ? t`Draft a follow-up email to the participants`
        : t`Draft a follow-up email for my most recent meeting.`,
    },
    {
      label: t`Find key decisions.`,
      icon: MagnifyingGlass,
      prompt: hasContext
        ? t`What were the key decisions that have been made?`
        : t`What key decisions were made in my recent meetings?`,
    },
  ];
  // General threads get no meeting suggestions: nothing from meetings is used.
  const suggestions =
    askScope === "general"
      ? []
      : isLiveMeeting
        ? liveMeetingSuggestions
        : meetingSuggestions;
  const threadNotice =
    askScope === "meeting"
      ? t`This meeting · separate thread. Answers use only this meeting.`
      : askScope === "library"
        ? t`All meetings · separate thread. Answers use excerpts from your meetings.`
        : askScope === "general"
          ? t`General · separate thread. No meeting content is used.`
          : null;

  const handleGoToSettings = useCallback(() => {
    openNew({ type: "settings", state: { tab: "intelligence" } });
  }, [openNew]);

  const handleSuggestionClick = useCallback(
    (prompt: string, askIntent?: AskExplicitIntent) => {
      onSendMessage?.(
        prompt,
        [{ type: "text", text: prompt }],
        undefined,
        askIntent ? { askIntent } : undefined,
      );
    },
    [onSendMessage],
  );

  if (!isModelConfigured) {
    return (
      <div className="flex justify-start py-2 pb-1">
        <div className="flex w-full flex-col">
          <div className="mb-2 flex items-center gap-2">
            <span
              className={cn([
                "text-sm font-medium",
                isDarkAppearance
                  ? "text-primary-foreground"
                  : "text-foreground",
              ])}
            >
              {APP_NAME} AI
            </span>
            <BetaChip isDarkAppearance={isDarkAppearance} />
          </div>
          <p
            className={cn([
              "mb-2 text-sm",
              isDarkAppearance
                ? "text-primary-foreground/80"
                : "text-muted-foreground",
            ])}
          >
            {t`Hi, I'm ${APP_NAME} AI. Set up a language model and I'll be ready to help.`}
          </p>
          <button
            onClick={handleGoToSettings}
            className={cn([
              "border-primary bg-primary text-primary-foreground inline-flex w-fit items-center gap-1.5 rounded-full border px-3 py-1.5 text-xs font-medium",
              "hover:bg-primary/90 shadow-[0_4px_14px_rgba(87,83,78,0.18)] transition-colors",
            ])}
          >
            <Sparkle size={12} />
            {t`Open AI Settings`}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex justify-start pb-1">
      <div className="flex w-full flex-col">
        {threadNotice ? (
          <p
            data-ask-thread-notice
            className={cn([
              "pb-1.5 text-xs",
              isDarkAppearance
                ? "text-primary-foreground/60"
                : "text-muted-foreground",
            ])}
          >
            {threadNotice}
          </p>
        ) : null}
        <div className="flex flex-col gap-0.5">
          {suggestions.map(({ label, icon: Icon, prompt, askIntent }) => (
            <button
              key={label}
              onClick={() => handleSuggestionClick(prompt, askIntent)}
              className={cn([
                "group grid w-full grid-cols-[1.5rem_minmax(0,1fr)] items-center gap-x-1.5 rounded-lg py-2 pr-3 pl-0 text-left text-sm",
                isDarkAppearance
                  ? "text-primary-foreground/85 hover:bg-primary-foreground/10"
                  : "text-muted-foreground hover:bg-muted/55",
                "transition-colors",
              ])}
            >
              <span className="flex size-6 items-center justify-center">
                <Icon
                  size={16}
                  className={cn([
                    "shrink-0 transition-colors",
                    isDarkAppearance
                      ? "text-primary-foreground/55 group-hover:text-primary-foreground/80"
                      : "text-muted-foreground/75 group-hover:text-foreground",
                  ])}
                />
              </span>
              <span className="min-w-0 truncate">{label}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function BetaChip({ isDarkAppearance }: { isDarkAppearance: boolean }) {
  return (
    <span
      className={cn([
        "rounded-full border px-1.5 py-0.5 text-[10px] font-medium",
        isDarkAppearance
          ? "border-border bg-accent text-accent-foreground"
          : "border-sky-200 bg-sky-100 text-sky-900",
      ])}
    >
      {t`Beta`}
    </span>
  );
}
