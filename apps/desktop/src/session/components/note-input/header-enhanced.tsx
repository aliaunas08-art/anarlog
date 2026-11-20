import { useLingui } from "@lingui/react/macro";
import { useCallback, useMemo } from "react";

import { CaretDown, Sparkle } from "@anlg/ui/components/icons";
import { Spinner } from "@anlg/ui/components/ui/spinner";
import { cn } from "@anlg/utils";

import {
  copyTextToClipboard,
  getEnhancedNoteTitle,
  getStoredNoteMarkdown,
  glassHeaderViewClassName,
  iconHeaderViewClassName,
  useHeaderViewVariant,
} from "./header-shared";
import {
  TemplatePickerPopover,
  type TemplateSelection,
} from "./template-picker";

import { useAITaskTask } from "~/ai/hooks";
import { getEnhancerService } from "~/services/enhancer";
import { useEnhancedNoteActions } from "~/session/components/note-input/enhanced-actions";
import { useEnhancedNote } from "~/session/queries";
import {
  type MenuItemDef,
  useNativeContextMenu,
} from "~/shared/hooks/useNativeContextMenu";
import { createTaskId } from "~/store/zustand/ai-task/task-configs";
import {
  TemplateIconGlyph,
  useUserTemplate,
  type TemplateIcon,
} from "~/templates";

export function HeaderViewEnhanced({
  isActive,
  onClick = () => {},
  sessionId,
  enhancedNoteId,
  canRemove = false,
  onRemove,
  onSelectNote,
}: {
  isActive: boolean;
  onClick?: () => void;
  sessionId: string;
  enhancedNoteId: string;
  canRemove?: boolean;
  onRemove?: () => void;
  onSelectNote?: (enhancedNoteId: string) => void;
}) {
  if (!isActive) {
    return (
      <HeaderViewEnhancedInactive
        enhancedNoteId={enhancedNoteId}
        onClick={onClick}
      />
    );
  }

  return (
    <HeaderViewEnhancedActive
      sessionId={sessionId}
      enhancedNoteId={enhancedNoteId}
      canRemove={canRemove}
      onRemove={onRemove}
      onSelectNote={onSelectNote}
    />
  );
}

function useEnhancedViewTitle(enhancedNoteId: string) {
  const { t } = useLingui();
  const enhancedNote = useEnhancedNote(enhancedNoteId);
  const rawTitle = enhancedNote?.title;
  const templateId = enhancedNote?.templateId;
  const { data: template } = useUserTemplate(templateId);
  const templateTitle = template?.title?.trim() || null;
  const templateIcon: TemplateIcon | null = template?.icon ?? null;
  const viewTitle = getEnhancedNoteTitle({
    rawTitle,
    templateTitle,
    templateId,
  });

  return {
    viewTitle,
    // The bottom bar names the default AI summary "Minutes"; template and
    // user-given titles are shown as-is.
    glassTitle:
      !templateTitle && viewTitle === "Summary" ? t`Minutes` : viewTitle,
    templateIcon,
    templateTooltip:
      templateId && templateTitle
        ? `${templateTitle} was used to generate this summary.`
        : undefined,
  };
}

function useEnhancedViewGenerating(enhancedNoteId: string) {
  const taskId = createTaskId(enhancedNoteId, "enhance");
  const enhanceTask = useAITaskTask(taskId, "enhance");

  return enhanceTask.isGenerating;
}

function HeaderViewEnhancedInactive({
  onClick = () => {},
  enhancedNoteId,
}: {
  enhancedNoteId: string;
  onClick?: () => void;
}) {
  const { viewTitle, glassTitle, templateIcon, templateTooltip } =
    useEnhancedViewTitle(enhancedNoteId);
  const isGenerating = useEnhancedViewGenerating(enhancedNoteId);
  const glass = useHeaderViewVariant() === "glass";

  if (glass) {
    return (
      <button
        data-main-area-window-drag-region
        data-tauri-drag-region="false"
        type="button"
        aria-label={glassTitle}
        onClick={onClick}
        title={templateTooltip}
        className={glassHeaderViewClassName(false)}
      >
        {isGenerating ? <Spinner size={14} className="shrink-0" /> : null}
        <span className="min-w-0 truncate">{glassTitle}</span>
      </button>
    );
  }

  return (
    <button
      data-main-area-window-drag-region
      data-tauri-drag-region="false"
      type="button"
      aria-label={viewTitle}
      onClick={onClick}
      title={templateTooltip}
      className={iconHeaderViewClassName(false, "tray", "px-2")}
    >
      {isGenerating ? (
        <Spinner size={16} className="shrink-0" />
      ) : templateIcon ? (
        <TemplateIconGlyph icon={templateIcon} className="size-4 text-sm" />
      ) : (
        <Sparkle className="size-4" />
      )}
    </button>
  );
}

function HeaderViewEnhancedActive({
  sessionId,
  enhancedNoteId,
  canRemove = false,
  onRemove,
  onSelectNote,
}: {
  sessionId: string;
  enhancedNoteId: string;
  canRemove?: boolean;
  onRemove?: () => void;
  onSelectNote?: (enhancedNoteId: string) => void;
}) {
  const { isGenerating, isError, onRegenerate } = useEnhanceLogic(
    sessionId,
    enhancedNoteId,
  );
  const enhancedNote = useEnhancedNote(enhancedNoteId);
  const content = enhancedNote?.content;
  const usedTemplateId = enhancedNote?.templateId?.trim() || null;
  const { viewTitle, glassTitle, templateIcon, templateTooltip } =
    useEnhancedViewTitle(enhancedNoteId);
  const glass = useHeaderViewVariant() === "glass";
  const noteMarkdown = useMemo(() => getStoredNoteMarkdown(content), [content]);

  const handleCopy = useCallback(() => {
    return copyTextToClipboard(noteMarkdown, {
      success: `${viewTitle} copied to clipboard`,
      error: `Failed to copy ${viewTitle}`,
    });
  }, [noteMarkdown, viewTitle]);
  const handleRegenerate = useCallback(() => {
    void onRegenerate(null);
  }, [onRegenerate]);
  const handleSelectTemplate = useCallback(
    (selection: TemplateSelection) => {
      if (isGenerating) {
        return;
      }

      const service = getEnhancerService();
      if (!service) {
        return;
      }

      onSelectNote?.(enhancedNoteId);

      void Promise.resolve(
        service.enhance(sessionId, {
          templateId: selection.templateId,
          targetNoteId: enhancedNoteId,
          templateTitle: selection.templateId ? selection.title : undefined,
        }),
      )
        .then((result) => {
          if (
            (result.type === "started" || result.type === "already_active") &&
            result.noteId !== enhancedNoteId
          ) {
            onSelectNote?.(result.noteId);
          }
        })
        .catch((error) => {
          console.error("[enhancer] failed to replace summary template", error);
        });
    },
    [enhancedNoteId, isGenerating, onSelectNote, sessionId],
  );
  const contextMenu = useMemo<MenuItemDef[]>(() => {
    const items: MenuItemDef[] = [
      {
        id: `copy-enhanced-${enhancedNoteId}`,
        text: "Copy",
        action: () => {
          void handleCopy();
        },
        disabled: noteMarkdown.length === 0,
      },
      {
        id: `regenerate-enhanced-${enhancedNoteId}`,
        text: "Regenerate",
        action: handleRegenerate,
        disabled: isGenerating,
      },
    ];

    if (canRemove) {
      items.push({ separator: true });
      items.push({
        id: `remove-enhanced-${enhancedNoteId}`,
        text: "Remove",
        action: () => {
          onRemove?.();
        },
        disabled: isGenerating || !onRemove,
      });
    }

    return items;
  }, [
    canRemove,
    enhancedNoteId,
    handleCopy,
    handleRegenerate,
    isGenerating,
    noteMarkdown.length,
    onRemove,
  ]);
  const showContextMenu = useNativeContextMenu(contextMenu);
  const templateMenuTrigger = glass ? (
    <button
      data-main-area-window-drag-region
      data-tauri-drag-region="false"
      type="button"
      aria-label={glassTitle}
      aria-current="page"
      aria-disabled={isGenerating}
      tabIndex={isGenerating ? -1 : 0}
      onClick={(event) => event.stopPropagation()}
      onPointerDown={(event) => event.stopPropagation()}
      onContextMenu={showContextMenu}
      title={templateTooltip}
      className={glassHeaderViewClassName(
        true,
        cn([
          "pr-3",
          isGenerating ? "cursor-not-allowed opacity-70" : "cursor-pointer",
          isError && "text-red-600 dark:text-red-400",
        ]),
      )}
    >
      {isGenerating ? <Spinner size={14} className="shrink-0" /> : null}
      <span className="min-w-0 truncate">{glassTitle}</span>
      <CaretDown aria-hidden className="size-3 opacity-60" />
    </button>
  ) : (
    <button
      data-main-area-window-drag-region
      data-tauri-drag-region="false"
      type="button"
      aria-label={viewTitle}
      aria-current="page"
      aria-disabled={isGenerating}
      tabIndex={isGenerating ? -1 : 0}
      onClick={(event) => event.stopPropagation()}
      onPointerDown={(event) => event.stopPropagation()}
      onContextMenu={showContextMenu}
      title={templateTooltip}
      className={iconHeaderViewClassName(
        true,
        "tray",
        cn([
          "max-w-56 min-w-[62px] gap-1.5 px-2 @max-[480px]:max-w-12 @max-[480px]:min-w-12 @max-[480px]:gap-0 @max-[480px]:px-1.5",
          isGenerating ? "cursor-not-allowed opacity-70" : "cursor-pointer",
          isError
            ? [
                "text-red-600 hover:bg-red-50 hover:text-red-700 focus-visible:bg-red-50",
                "dark:text-red-400 dark:hover:bg-red-950/50 dark:hover:text-red-300 dark:focus-visible:bg-red-950/50",
              ]
            : [
                "focus-visible:text-foreground focus-visible:bg-white",
                "dark:focus-visible:text-primary dark:focus-visible:bg-white",
              ],
        ]),
      )}
    >
      {isGenerating ? (
        <Spinner size={16} className="shrink-0" />
      ) : templateIcon ? (
        <TemplateIconGlyph icon={templateIcon} className="size-4 text-sm" />
      ) : (
        <Sparkle className="size-4" />
      )}
      <span className="min-w-0 truncate text-xs font-medium @max-[480px]:sr-only">
        {viewTitle}
      </span>
      <CaretDown className="size-3.5" />
    </button>
  );

  return (
    <TemplatePickerPopover
      onSelectTemplate={handleSelectTemplate}
      usedTemplateId={usedTemplateId}
      onRegenerateUsed={handleRegenerate}
      isRegenerating={isGenerating}
      trigger={templateMenuTrigger}
    />
  );
}

const useEnhanceLogic = (sessionId: string, enhancedNoteId: string) =>
  useEnhancedNoteActions({ sessionId, enhancedNoteId });
