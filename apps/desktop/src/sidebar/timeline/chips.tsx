import type { ReactNode } from "react";

import { ArrowUp } from "@anlg/ui/components/icons";
import { Button } from "@anlg/ui/components/ui/button";
import { cn } from "@anlg/utils";

export function UpcomingMeetingChip({
  ariaLabel,
  label,
  onClick,
}: {
  ariaLabel: string;
  label: string;
  onClick: () => void;
}) {
  return (
    <TimelineTopChip
      aria-live="polite"
      ariaLabel={ariaLabel}
      data-sidebar-upcoming-meeting-status
      className="border-destructive bg-destructive text-destructive-foreground w-28 justify-center shadow-md"
      icon={<ArrowUp aria-hidden className="size-3" weight="bold" />}
      onClick={onClick}
    >
      {label}
    </TimelineTopChip>
  );
}

export function TimelineTopChip({
  ariaLabel,
  children,
  icon,
  onClick,
  ...props
}: {
  ariaLabel?: string;
  children: ReactNode;
  icon: ReactNode;
  className?: string;
  role?: string;
  "aria-live"?: "off" | "polite" | "assertive";
  "data-sidebar-upcoming-meeting-status"?: true;
  onClick?: () => void;
}) {
  const className = cn([
    "border-border bg-card text-muted-foreground flex h-6 items-center gap-1 rounded-full border px-2.5 text-xs font-medium shadow-xs",
    onClick && "hover:bg-accent hover:text-foreground transition-colors",
    "focus-visible:ring-ring focus-visible:ring-2 focus-visible:outline-hidden",
    props.className,
  ]);

  if (onClick) {
    return (
      <Button
        {...props}
        aria-label={ariaLabel}
        className={className}
        onClick={onClick}
        size="sm"
        variant="outline"
      >
        <span className="flex size-3 shrink-0 items-center justify-center">
          {icon}
        </span>
        <span className="truncate">{children}</span>
      </Button>
    );
  }

  return (
    <div {...props} aria-label={ariaLabel} className={className}>
      <span className="flex size-3 shrink-0 items-center justify-center">
        {icon}
      </span>
      <span className="truncate">{children}</span>
    </div>
  );
}
