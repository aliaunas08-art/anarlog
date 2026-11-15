import { useLingui } from "@lingui/react/macro";

import { cn } from "@anlg/utils";

import { ASK_SCOPES, type AskScope } from "~/chat/guardrails/scope";

export function useAskScopeLabels(): Record<AskScope, string> {
  const { t } = useLingui();
  return {
    meeting: t`This meeting`,
    library: t`All meetings`,
    general: t`General`,
  };
}

/**
 * Segmented [ This meeting | All meetings | General ] control, in the bottom
 * bar's Liquid Glass style with a sliding selection thumb.
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
  const index = ASK_SCOPES.indexOf(value);

  return (
    <div
      role="radiogroup"
      aria-label={t`Ask scope`}
      data-tauri-drag-region="false"
      data-ask-scope-control
      className={cn([
        "cf-glass rounded-pill relative grid h-7 shrink-0 grid-cols-3 items-center p-[2px] [corner-shape:round]",
        className,
      ])}
    >
      <span
        aria-hidden="true"
        className="cf-bar-thumb rounded-pill pointer-events-none absolute top-[2px] bottom-[2px] left-[2px] transition-transform duration-300 ease-out [corner-shape:round] motion-reduce:transition-none"
        style={{
          width: "calc((100% - 4px) / 3)",
          transform: `translateX(${index * 100}%)`,
        }}
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
              "cf-motion rounded-pill relative z-10 h-6 min-w-0 truncate px-2.5 text-[12px] whitespace-nowrap transition-colors select-none [corner-shape:round]",
              "focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-none",
              selected
                ? "font-medium text-[var(--cf-label)]"
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
