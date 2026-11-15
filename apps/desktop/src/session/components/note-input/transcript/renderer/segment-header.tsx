import { Check } from "@anlg/ui/components/icons";
import { cn } from "@anlg/utils";

import { useTranscriptSelectionState } from "./selection-context";
import { SpeakerAssignPopover } from "./speaker-assign";
import { useSegmentColorVars } from "./utils";

import type { Segment } from "~/stt/live-segment";

export function SegmentHeader({
  segment,
  transcriptId,
  sessionId,
  label,
  selected = false,
}: {
  segment: Segment;
  transcriptId: string;
  sessionId?: string;
  label: string;
  selected?: boolean;
}) {
  const { selectMode } = useTranscriptSelectionState();
  const colorVars = useSegmentColorVars(segment.key);
  const headerClassName = cn([
    "relative py-1",
    "text-xs font-light",
    "flex items-center gap-2",
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
        className="rounded-pill size-2 shrink-0 bg-[var(--speaker-dot)]"
      />
      <SpeakerAssignPopover
        segment={segment}
        transcriptId={transcriptId}
        sessionId={sessionId}
        color="var(--cf-label)"
        className="-ml-1 font-medium"
        label={label}
      />
    </div>
  );
}
