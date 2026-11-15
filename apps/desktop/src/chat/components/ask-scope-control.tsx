import { useLingui } from "@lingui/react/macro";
import { motion, useReducedMotion } from "motion/react";

import { cn } from "@anlg/utils";

import { ASK_SCOPES, type AskScope } from "~/chat/guardrails/scope";

export function useAskScopeLabels(): Record<AskScope, string> {
  const { t } = useLingui();
  return {
    meeting: t`This meeting`,
    library: t`All meetings`,
  };
}

/**
 * Segmented [ This meeting | All meetings ] control: a flat fill with a
 * hairline and a solid sliding thumb, like the Record / New note buttons.
 */
export function AskScopeControl({
  value,
  meetingAvailable,
  onChange,
  className,
}: {
  value: AskScope;
  meetingAvailable: boolean;
  onChange: (scope: AskScope) => void;
  className?: string;
}) {
  const { t } = useLingui();
  const labels = useAskScopeLabels();
  const reduceMotion = useReducedMotion();
  const index = Math.max(0, ASK_SCOPES.indexOf(value));

  return (
    <div
      role="radiogroup"
      aria-label={t`Ask scope`}
      data-tauri-drag-region="false"
      data-ask-scope-control
      className={cn([
        "cf-hairline rounded-pill relative grid h-7 shrink-0 grid-cols-2 items-center bg-[var(--cf-fill)] p-[2px] [corner-shape:round]",
        className,
      ])}
    >
      <motion.span
        aria-hidden="true"
        className="cf-bar-thumb rounded-pill pointer-events-none absolute top-[2px] bottom-[2px] left-[2px] [corner-shape:round]"
        style={{ width: `calc((100% - 4px) / ${ASK_SCOPES.length})` }}
        initial={false}
        animate={{ x: `${index * 100}%` }}
        transition={
          reduceMotion
            ? { duration: 0 }
            : { type: "spring", bounce: 0, duration: 0.3 }
        }
      />
      {ASK_SCOPES.map((scope) => {
        const disabled = scope === "meeting" && !meetingAvailable;
        const selected = scope === value;
        return (
          <button
            key={scope}
            type="button"
            role="radio"
            aria-checked={selected}
            disabled={disabled}
            title={disabled ? t`Open a meeting to ask about it` : undefined}
            onClick={() => onChange(scope)}
            className={cn([
              "cf-motion rounded-pill relative z-10 h-6 min-w-0 truncate px-3 text-[12px] whitespace-nowrap transition-colors select-none [corner-shape:round]",
              "focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-none",
              selected
                ? "font-medium text-[var(--cf-window)]"
                : "text-[var(--cf-secondary)] hover:text-[var(--cf-label)]",
              "disabled:cursor-default disabled:opacity-45 disabled:hover:text-[var(--cf-secondary)]",
            ])}
          >
            {labels[scope]}
          </button>
        );
      })}
    </div>
  );
}
