import type { FloatingBarState } from "@anlg/plugin-windows";

export function DictationTranscript({
  dictation,
  color,
}: {
  dictation: NonNullable<FloatingBarState["dictation"]>;
  /** Text colour: the bar's label colour for its colour scheme. */
  color: string;
}) {
  const status =
    dictation.phase === "transcribing"
      ? "Finishing…"
      : dictation.phase === "starting"
        ? "Starting…"
        : "Listening…";
  const empty = dictation.previewUnavailable
    ? "Live preview is unavailable. Your text will appear when you finish."
    : !dictation.previewEnabled
      ? "Enable Live transcript preview in Settings → Dictation to see words as you speak."
      : status;

  return (
    <p
      className="px-1 text-[15px] leading-6 break-words whitespace-pre-wrap"
      style={{ color }}
      aria-live="polite"
      aria-atomic="false"
    >
      {dictation.text || dictation.partial ? (
        <>
          {dictation.text}
          {dictation.text && dictation.partial ? " " : ""}
          <span className="opacity-60">{dictation.partial}</span>
        </>
      ) : (
        <span className="opacity-60">{empty}</span>
      )}
    </p>
  );
}
