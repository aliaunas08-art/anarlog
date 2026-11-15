import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { FloatingBarState } from "@anlg/plugin-windows";

import { FloatingBarOverlay } from "./bar";

const motion = vi.hoisted(() => ({ reduced: false }));

vi.mock("motion/react", () => ({
  useReducedMotion: () => motion.reduced,
}));

function state(overrides: Partial<FloatingBarState> = {}): FloatingBarState {
  return {
    dictation: null,
    amplitude: 0.4,
    elapsedSeconds: 754,
    title: "Weekly sync",
    status: "recording",
    colorScheme: "light",
    opacity: 0.78,
    liveCaptionOpacity: 0.3,
    liveCaptionWidth: 440,
    liveCaptionLineCount: 1,
    liveCaptionPosition: "topCenter",
    liveCaptionMinimized: true,
    liveCaptionToggleVisible: true,
    transcriptBubbles: [
      {
        id: "1",
        speakerLabel: "Ada",
        text: "Let's start.",
        isSelf: false,
        isFinal: true,
        startMs: 0,
        endMs: 1200,
        overlapsPrevious: false,
        overlapsNext: false,
      },
    ],
    ...overrides,
  };
}

describe("FloatingBarOverlay", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    motion.reduced = false;
  });

  function meterHeights(container: HTMLElement) {
    const meter = container.querySelector("[data-floating-bar-meter]");
    expect(meter).not.toBeNull();
    return Array.from(
      meter!.querySelectorAll<HTMLElement>("[data-level-meter-bar]"),
    ).map((bar) => bar.style.height);
  }

  it("shows the shared level meter fed by the bar's amplitude", () => {
    vi.useFakeTimers();
    const props = { onStop: vi.fn(), onToggleExpanded: vi.fn() };
    const view = render(
      <FloatingBarOverlay state={state({ amplitude: 0 })} {...props} />,
    );

    const meter = view.container.querySelector("[data-floating-bar-meter]");
    expect(meter?.hasAttribute("data-level-meter")).toBe(true);
    expect(meterHeights(view.container)).toHaveLength(10);
    expect(new Set(meterHeights(view.container))).toEqual(new Set(["3px"]));

    // Amplitude events update `state.amplitude`; the newest bar is on the right.
    view.rerender(
      <FloatingBarOverlay state={state({ amplitude: 1 })} {...props} />,
    );
    act(() => {
      vi.advanceTimersByTime(130);
    });
    const heights = meterHeights(view.container);
    expect(heights[heights.length - 1]).toBe("16px");
    expect(heights[0]).toBe("3px");
  });

  it("keeps the meter still with reduced motion", () => {
    vi.useFakeTimers();
    motion.reduced = true;
    const props = { onStop: vi.fn(), onToggleExpanded: vi.fn() };
    const view = render(
      <FloatingBarOverlay state={state({ amplitude: 0 })} {...props} />,
    );
    const before = meterHeights(view.container);

    view.rerender(
      <FloatingBarOverlay state={state({ amplitude: 1 })} {...props} />,
    );
    act(() => {
      vi.advanceTimersByTime(1000);
    });

    expect(meterHeights(view.container)).toEqual(before);
    expect(
      view.container
        .querySelector("[data-floating-bar-meter]")
        ?.getAttribute("data-static"),
    ).toBe("true");
  });

  it("shows a live dot, a tabular timer and a neutral Stop", () => {
    const view = render(
      <FloatingBarOverlay
        state={state({ colorScheme: "dark" })}
        onStop={vi.fn()}
        onToggleExpanded={vi.fn()}
      />,
    );

    const dot = view.container.querySelector("[data-floating-bar-live-dot]");
    expect(dot?.classList.contains("cf-record-dot")).toBe(true);

    const timer = screen.getByRole("timer");
    expect(timer.textContent).toBe("12:34");
    expect(timer.className).toContain("tabular-nums");
    expect(timer.getAttribute("aria-label")).toBe("Recording time 12:34");

    const stop = screen.getByRole("button", { name: "Stop listening" });
    expect(stop.textContent).toBe("Stop");
    // Label-colour fill with window-colour text, like the in-app pill.
    expect(stop.style.background).toBe("rgb(245, 245, 247)");
    expect(stop.style.color).toBe("rgb(0, 0, 0)");

    const surface = view.container.querySelector<HTMLElement>(
      "[data-floating-bar-surface]",
    );
    expect(surface?.style.boxShadow).toContain("rgba(255, 255, 255, 0.1)");
  });

  it("uses a white pill in light mode", () => {
    const view = render(
      <FloatingBarOverlay
        state={state({ colorScheme: "light", opacity: 0.95 })}
        onStop={vi.fn()}
        onToggleExpanded={vi.fn()}
      />,
    );

    const surface = view.container.querySelector<HTMLElement>(
      "[data-floating-bar-surface]",
    );
    expect(surface?.style.background).toBe("rgb(255, 255, 255)");
    const stop = screen.getByRole("button", { name: "Stop listening" });
    expect(stop.style.background).toBe("rgb(29, 29, 31)");
  });

  it("omits the timer without elapsed time", () => {
    render(
      <FloatingBarOverlay
        state={state({ elapsedSeconds: null })}
        onStop={vi.fn()}
        onToggleExpanded={vi.fn()}
      />,
    );

    expect(screen.queryByRole("timer")).toBeNull();
  });

  it("draws transcript bubbles on a neutral surface", () => {
    const view = render(
      <FloatingBarOverlay
        state={state({ colorScheme: "dark", liveCaptionMinimized: false })}
        onStop={vi.fn()}
        onToggleExpanded={vi.fn()}
      />,
    );

    const bubble = view.container.querySelector<HTMLElement>(
      "[data-floating-transcript-bubble]",
    );
    expect(bubble?.style.background).toBe("rgba(255, 255, 255, 0.08)");
    expect(bubble?.style.color).toBe("rgb(245, 245, 247)");
  });

  it.each(["recording", "reconnecting", "error"] as const)(
    "stops listening while %s",
    (status) => {
      const onStop = vi.fn();

      render(
        <FloatingBarOverlay
          state={state({ status })}
          onStop={onStop}
          onToggleExpanded={vi.fn()}
        />,
      );

      fireEvent.click(screen.getByRole("button", { name: /stop listening/i }));

      expect(onStop).toHaveBeenCalledOnce();
    },
  );

  it("expands to the live transcript and can collapse again", () => {
    const onToggleExpanded = vi.fn();

    const view = render(
      <FloatingBarOverlay
        state={state()}
        onStop={vi.fn()}
        onToggleExpanded={onToggleExpanded}
      />,
    );

    const toggle = screen.getByRole("button", {
      name: "Expand live transcript",
    });
    fireEvent.click(toggle);
    expect(onToggleExpanded).toHaveBeenCalledWith(true);

    view.rerender(
      <FloatingBarOverlay
        state={state({ liveCaptionMinimized: false })}
        onStop={vi.fn()}
        onToggleExpanded={onToggleExpanded}
      />,
    );

    screen.getByRole("button", { name: "Collapse live transcript" });
    expect(screen.getByText("Let's start.")).toBeTruthy();

    fireEvent.click(
      screen.getByRole("button", { name: "Collapse live transcript" }),
    );
    expect(onToggleExpanded).toHaveBeenCalledWith(false);
  });
});
