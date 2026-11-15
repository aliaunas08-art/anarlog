import { useReducedMotion } from "motion/react";
import { type HTMLAttributes, useEffect, useRef, useState } from "react";

import { cn } from "@anlg/utils";

export const LEVEL_METER_BARS = 14;
export const LEVEL_METER_SAMPLE_MS = 120;
/** Bar height (0–1) drawn for every bar when motion is reduced. */
const STATIC_LEVEL = 0.3;

/**
 * Live input level meter: a rolling row of thin bars, newest on the right.
 * Shared by the in-app recording pill and the floating recording bar so both
 * read the same. `level` is 0–1 (the listener's combined mic + speaker
 * amplitude). With reduced motion the bars stay still at a fixed height.
 */
export function LevelMeter({
  level,
  bars = LEVEL_METER_BARS,
  height = 18,
  color = "var(--cf-label)",
  muted = false,
  className,
  ...rest
}: {
  level: number;
  bars?: number;
  /** Tallest bar, in px. */
  height?: number;
  color?: string;
  muted?: boolean;
  className?: string;
} & Omit<HTMLAttributes<HTMLSpanElement>, "color">) {
  const reducedMotion = useReducedMotion() ?? false;
  const history = useLevelHistory(level, bars, !reducedMotion);
  const values = reducedMotion ? history.map(() => STATIC_LEVEL) : history;

  return (
    <span
      aria-hidden="true"
      data-level-meter
      data-static={reducedMotion || undefined}
      className={cn([
        "flex items-center gap-[2px]",
        muted && "opacity-40",
        className,
      ])}
      style={{ height }}
      {...rest}
    >
      {values.map((value, index) => (
        <span
          key={index}
          data-level-meter-bar
          className="w-[2px] shrink-0 rounded-[1px]"
          style={{
            background: color,
            height: `${Math.max(3, Math.round(value * height))}px`,
            opacity: 0.35 + value * 0.65,
          }}
        />
      ))}
    </span>
  );
}

/** Rolling window of recent input levels (newest on the right). */
export function useLevelHistory(level: number, bars: number, live = true) {
  const levelRef = useRef(level);
  levelRef.current = level;
  const [history, setHistory] = useState<number[]>(() =>
    Array.from({ length: bars }, () => 0),
  );

  useEffect(() => {
    if (!live) {
      return;
    }
    const id = window.setInterval(() => {
      setHistory((previous) => [
        ...previous.slice(Math.max(previous.length - (bars - 1), 0)),
        Math.min(Math.max(levelRef.current, 0), 1),
      ]);
    }, LEVEL_METER_SAMPLE_MS);
    return () => window.clearInterval(id);
  }, [bars, live]);

  return history.length === bars
    ? history
    : Array.from({ length: bars }, (_, index) => history[index] ?? 0);
}
