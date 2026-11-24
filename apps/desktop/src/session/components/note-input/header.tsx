import { useLingui } from "@lingui/react/macro";
import { motion, useReducedMotion } from "motion/react";
import { useLayoutEffect, useRef, useState } from "react";

import { cn } from "@anlg/utils";
import { createEditorTabs } from "@anlg/utils/session";

export { createEditorTabs } from "@anlg/utils/session";

import { HeaderViewEnhanced } from "./header-enhanced";
import { HeaderViewRaw } from "./header-raw";
import {
  type HeaderViewVariant,
  HeaderViewVariantContext,
} from "./header-shared";
import { HeaderViewShared } from "./header-shared-note";
import { HeaderViewTranscript } from "./header-transcript";

import { FolderPicker } from "~/session/components/folder-picker";
import {
  useCanShowTranscript,
  useHasTranscript,
} from "~/session/components/shared";
import { useEnhancedNotes } from "~/session/hooks/useEnhancedNotes";
import { deleteEnhancedNote } from "~/session/queries";
import { useSessionSharedNoteIds } from "~/shared-notes/event-keys";
import { type EditorView } from "~/store/zustand/tabs/schema";
import { useListener } from "~/stt/contexts";

export function Header({ sessionId }: { sessionId: string }) {
  return <FolderPicker sessionId={sessionId} align="end" />;
}

export function SessionViewSwitcher({
  sessionId,
  editorTabs,
  currentTab,
  handleTabChange,
  isTranscribing = false,
  transcriptEditMode = false,
  onTranscriptEditModeChange,
  variant = "tray",
}: {
  sessionId: string;
  editorTabs: EditorView[];
  currentTab: EditorView;
  handleTabChange: (view: EditorView) => void;
  isTranscribing?: boolean;
  transcriptEditMode?: boolean;
  onTranscriptEditModeChange?: (editMode: boolean) => void;
  variant?: HeaderViewVariant;
}) {
  const { t } = useLingui();
  const groupRef = useRef<HTMLDivElement>(null);
  const thumb = useSelectionThumb(groupRef, variant === "glass", currentTab);
  const reduceMotion = useReducedMotion();
  const primaryEnhancedTabId = editorTabs.find(
    (view): view is Extract<EditorView, { type: "enhanced" }> =>
      view.type === "enhanced",
  )?.id;
  const shouldUseViewSwitcher = editorTabs.length > 1;

  if (!shouldUseViewSwitcher) {
    return null;
  }

  return (
    <HeaderViewVariantContext.Provider value={variant}>
      <div
        ref={groupRef}
        role="group"
        aria-label={t`Session note views`}
        data-tauri-drag-region="false"
        data-variant={variant}
        className={cn([
          "pointer-events-auto relative z-10 w-fit max-w-full shrink-0 overflow-visible",
          variant === "glass"
            ? // Segments of the bottom bar's single flat capsule; the
              // selection is one solid thumb that slides between them.
              "flex h-9 items-center gap-[2px]"
            : "bg-foreground/10 dark:bg-accent/55 rounded-pill flex h-7 items-center gap-[2px] p-[2px] [corner-shape:round]",
        ])}
      >
        {variant === "glass" && thumb ? (
          <motion.span
            aria-hidden="true"
            data-session-view-thumb
            className="cf-bar-thumb rounded-pill pointer-events-none absolute top-0 left-0 h-9 [corner-shape:round]"
            initial={false}
            animate={{ x: thumb.x, width: thumb.width }}
            transition={
              reduceMotion || thumb.instant
                ? { duration: 0 }
                : { type: "spring", bounce: 0, duration: 0.3 }
            }
          />
        ) : null}
        {editorTabs.map((view, index) => {
          if (view.type === "enhanced") {
            return (
              <HeaderViewEnhanced
                key={`enhanced-${view.id}`}
                sessionId={sessionId}
                enhancedNoteId={view.id}
                canRemove={view.id !== primaryEnhancedTabId}
                onRemove={
                  view.id !== primaryEnhancedTabId
                    ? () => {
                        const previousView = editorTabs[index - 1];
                        if (
                          currentTab.type === "enhanced" &&
                          currentTab.id === view.id &&
                          previousView
                        ) {
                          handleTabChange(previousView);
                        }

                        void deleteEnhancedNote(view.id).catch((error) => {
                          console.error(
                            "[session-header] failed to remove summary",
                            error,
                          );
                        });
                      }
                    : undefined
                }
                onSelectNote={(enhancedNoteId) =>
                  handleTabChange({ type: "enhanced", id: enhancedNoteId })
                }
                isActive={
                  currentTab.type === "enhanced" && currentTab.id === view.id
                }
                onClick={() => handleTabChange(view)}
              />
            );
          }

          if (view.type === "raw") {
            return (
              <HeaderViewRaw
                key={view.type}
                sessionId={sessionId}
                isActive={currentTab.type === view.type}
                standalone={!shouldUseViewSwitcher}
                onClick={() => handleTabChange(view)}
              />
            );
          }

          if (view.type === "transcript") {
            return (
              <HeaderViewTranscript
                key={view.type}
                sessionId={sessionId}
                isActive={currentTab.type === view.type}
                isTranscribing={isTranscribing}
                editMode={transcriptEditMode}
                onEditModeChange={onTranscriptEditModeChange}
                onClick={() => handleTabChange(view)}
              />
            );
          }

          if (view.type === "shared") {
            return (
              <HeaderViewShared
                key={`shared-${view.id}`}
                isActive={
                  currentTab.type === "shared" && currentTab.id === view.id
                }
                onClick={() => handleTabChange(view)}
              />
            );
          }

          return null;
        })}
      </div>
    </HeaderViewVariantContext.Provider>
  );
}

type ThumbRect = { x: number; width: number; instant: boolean };

/**
 * Measures the active segment (`aria-current="page"`) so the solid thumb can
 * slide to it. The first placement and resizes jump; view changes animate.
 */
function useSelectionThumb(
  groupRef: React.RefObject<HTMLDivElement | null>,
  enabled: boolean,
  currentTab: EditorView,
): ThumbRect | null {
  const [rect, setRect] = useState<ThumbRect | null>(null);
  const tabKey =
    currentTab.type === "enhanced" || currentTab.type === "shared"
      ? `${currentTab.type}:${currentTab.id}`
      : currentTab.type;
  const lastKeyRef = useRef<string | null>(null);

  useLayoutEffect(() => {
    const group = groupRef.current;
    if (!enabled || !group) {
      setRect(null);
      return;
    }
    const measure = (instant: boolean) => {
      const active = group.querySelector<HTMLElement>('[aria-current="page"]');
      if (!active) {
        setRect(null);
        return;
      }
      const groupBox = group.getBoundingClientRect();
      const box = active.getBoundingClientRect();
      if (box.width === 0) {
        setRect(null);
        return;
      }
      const next = {
        x: Math.round(box.left - groupBox.left),
        width: Math.round(box.width),
      };
      setRect((previous) =>
        previous &&
        previous.x === next.x &&
        previous.width === next.width &&
        previous.instant === instant
          ? previous
          : { ...next, instant },
      );
    };
    const viewChanged =
      lastKeyRef.current !== null && lastKeyRef.current !== tabKey;
    lastKeyRef.current = tabKey;
    measure(!viewChanged);

    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => measure(true));
    observer.observe(group);
    for (const child of Array.from(group.querySelectorAll("button"))) {
      observer.observe(child);
    }
    return () => observer.disconnect();
  }, [enabled, groupRef, tabKey]);

  return rect;
}

export function useEditorTabs({
  audioExists = false,
  sessionId,
}: {
  audioExists?: boolean;
  sessionId: string;
}): EditorView[] {
  const canShowTranscript = useCanShowTranscript(sessionId, { audioExists });
  const hasTranscript = useHasTranscript(sessionId);
  const sessionMode = useListener((state) => state.getSessionMode(sessionId));
  const enhancedNoteIds = useEnhancedNotes(
    sessionId,
    hasTranscript && sessionMode === "inactive",
  );

  const sharedNoteIds = useSessionSharedNoteIds(sessionId);

  return createEditorTabs({
    enhancedNoteIds,
    canShowTranscript,
    sharedNoteIds,
  });
}
