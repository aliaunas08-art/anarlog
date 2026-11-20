import { useLingui } from "@lingui/react/macro";

import { Check } from "@anlg/ui/components/icons";
import { cn } from "@anlg/utils";

import { useTranscriptSelectionState } from "./selection-context";
import { SpeakerAssignPopover } from "./speaker-assign";
import { useSegmentColorVars } from "./utils";

import { isSegmentSpeakerUncertain } from "~/speakers/attribution";
import type { Segment } from "~/stt/live-segment";

export function SegmentHeader({
  segment,
  transcriptId,
  sessionId,
  label,
  selected = false,
  audioOffsetMs = 0,
}: {
  segment: Segment;
  transcriptId: string;
  sessionId?: string;
  label: string;
  selected?: boolean;
  audioOffsetMs?: number;
}) {
  const { t } = useLingui();
  const { selectMode } = useTranscriptSelectionState();
  // Uncertain attributions stay quiet: a muted label and a tooltip.
  const uncertain = isSegmentSpeakerUncertain(segment);
  const colorVars = useSegmentColorVars(segment.key);
  const headerClassName = cn([
    // Board speaker line: 6px dot, 11px semibold secondary label.
    "relative py-0.5",
    "text-[11px] font-semibold",
    "flex items-center gap-1.5",
    "[--segment-color:var(--segment-color-light)]",
    "dark:[--segment-color:var(--segment-color-dark)]",
    "[--speaker-dot:var(--speaker-dot-light)]",
    "dark:[--speaker-dot:var(--speaker-dot-dark)]",
  ]);

  return (
    <div className={headerClassName} style={colorVars}>
      {selectMode ? (
        <span
          aria-hidden="true"
          className={cn([
            "flex size-4 shrink-0 items-center justify-center rounded-full border",
            selected
              ? "border-primary bg-primary text-primary-foreground"
              : "border-muted-foreground/40",
          ])}
        >
          {selected ? <Check className="size-2.5" weight="bold" /> : null}
        </span>
      ) : null}
      <span
        aria-hidden="true"
        data-speaker-dot
        className="rounded-pill size-1.5 shrink-0 bg-[var(--speaker-dot)]"
      />
      <SpeakerAssignPopover
        segment={segment}
        transcriptId={transcriptId}
        sessionId={sessionId}
        color={uncertain ? "var(--cf-tertiary)" : "var(--cf-secondary)"}
        className="-ml-1 font-semibold"
        label={label}
        title={uncertain ? t`Speaker uncertain` : undefined}
        uncertain={uncertain}
        audioOffsetMs={audioOffsetMs}
      />
    </div>
  );
}
