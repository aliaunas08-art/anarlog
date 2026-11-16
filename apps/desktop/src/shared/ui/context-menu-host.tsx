import { motion, useReducedMotion } from "motion/react";
import {
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { createPortal } from "react-dom";

import { cn } from "@anlg/utils";

import {
  closeContextMenu,
  getContextMenu,
  type MenuItemDef,
  type OpenContextMenu,
  subscribeContextMenu,
} from "~/shared/hooks/useNativeContextMenu";

const VIEWPORT_MARGIN_PX = 8;

type ActionItem = Extract<MenuItemDef, { id: string }>;

function isActionable(item: MenuItemDef): item is ActionItem {
  return !("separator" in item) && !item.disabled;
}

function isDestructive(item: ActionItem) {
  return item.destructive ?? item.id.startsWith("delete");
}

const ACCELERATOR_GLYPHS: Record<string, string> = {
  cmdorctrl: "⌘",
  commandorcontrol: "⌘",
  cmd: "⌘",
  command: "⌘",
  super: "⌘",
  ctrl: "⌃",
  control: "⌃",
  shift: "⇧",
  alt: "⌥",
  option: "⌥",
  backspace: "⌫",
  delete: "⌦",
  enter: "↩",
  return: "↩",
  escape: "⎋",
  esc: "⎋",
  tab: "⇥",
  up: "↑",
  down: "↓",
  left: "←",
  right: "→",
};

export function formatAccelerator(accelerator: string) {
  return accelerator
    .split("+")
    .map((part) => {
      const glyph = ACCELERATOR_GLYPHS[part.trim().toLowerCase()];
      return glyph ?? part.trim().toUpperCase();
    })
    .join("");
}

/**
 * Renders the right-click menu opened by `useNativeContextMenu`. Mount once
 * per window.
 */
export function ContextMenuHost() {
  const menu = useSyncExternalStore(
    subscribeContextMenu,
    getContextMenu,
    getContextMenu,
  );
  if (!menu) return null;
  return createPortal(
    <ContextMenu key={menu.key} menu={menu} />,
    document.body,
  );
}

function ContextMenu({ menu }: { menu: OpenContextMenu }) {
  const panelRef = useRef<HTMLDivElement>(null);
  const reduceMotion = useReducedMotion() ?? false;
  const [active, setActive] = useState(-1);
  const [placement, setPlacement] = useState<{
    left: number;
    top: number;
    origin: string;
  } | null>(null);

  // Place at the pointer, flipping left or up rather than leaving the window.
  useLayoutEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    const { width, height } = panel.getBoundingClientRect();
    const maxLeft = window.innerWidth - VIEWPORT_MARGIN_PX - width;
    const maxTop = window.innerHeight - VIEWPORT_MARGIN_PX - height;
    const flipX = menu.x > maxLeft && menu.x - width >= VIEWPORT_MARGIN_PX;
    const flipY = menu.y > maxTop && menu.y - height >= VIEWPORT_MARGIN_PX;
    const left = Math.max(
      VIEWPORT_MARGIN_PX,
      Math.min(flipX ? menu.x - width : menu.x, maxLeft),
    );
    const top = Math.max(
      VIEWPORT_MARGIN_PX,
      Math.min(flipY ? menu.y - height : menu.y, maxTop),
    );
    setPlacement({
      left,
      top,
      origin: `${flipX ? "right" : "left"} ${flipY ? "bottom" : "top"}`,
    });
    panel.focus({ preventScroll: true });
  }, [menu]);

  useEffect(() => {
    const close = () => closeContextMenu();
    const onScroll = (event: Event) => {
      if (panelRef.current?.contains(event.target as Node)) return;
      close();
    };
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", onScroll, true);
    const returnFocus = menu.returnFocus;
    return () => {
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", onScroll, true);
      if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
    };
  }, [menu]);

  const run = (item: MenuItemDef) => {
    if (!isActionable(item)) return;
    closeContextMenu();
    item.action();
  };

  const move = (from: number, step: 1 | -1) => {
    const { items } = menu;
    for (let i = 1; i <= items.length; i++) {
      const index = (from + step * i + items.length * 2) % items.length;
      if (isActionable(items[index])) return index;
    }
    return -1;
  };

  const onKeyDown = (event: ReactKeyboardEvent) => {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        setActive((index) => move(index < 0 ? -1 : index, 1));
        break;
      case "ArrowUp":
        event.preventDefault();
        setActive((index) => move(index < 0 ? menu.items.length : index, -1));
        break;
      case "Home":
        event.preventDefault();
        setActive(move(-1, 1));
        break;
      case "End":
        event.preventDefault();
        setActive(move(menu.items.length, -1));
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        if (active >= 0) run(menu.items[active]);
        break;
      case "Escape":
      case "Tab":
        event.preventDefault();
        closeContextMenu();
        break;
    }
  };

  return (
    <div
      data-slot="context-menu-layer"
      className="fixed inset-0 z-[100]"
      // A press outside closes the menu and is swallowed, like a native menu.
      // The layer unmounts before a right-click's `contextmenu` fires, so that
      // event reaches the element underneath and can open its own menu.
      onPointerDown={(event) => {
        if (!panelRef.current?.contains(event.target as Node))
          closeContextMenu();
      }}
      onContextMenu={(event) => event.preventDefault()}
    >
      <motion.div
        ref={panelRef}
        role="menu"
        tabIndex={-1}
        aria-activedescendant={
          active >= 0 ? `context-menu-item-${menu.key}-${active}` : undefined
        }
        data-slot="context-menu"
        onKeyDown={onKeyDown}
        onPointerLeave={() => setActive(-1)}
        initial={reduceMotion ? false : { opacity: 0, scale: 0.96 }}
        animate={{ opacity: placement ? 1 : 0, scale: placement ? 1 : 0.96 }}
        transition={
          reduceMotion
            ? { duration: 0 }
            : { type: "spring", bounce: 0, duration: 0.22 }
        }
        style={{
          left: placement?.left ?? menu.x,
          top: placement?.top ?? menu.y,
          transformOrigin: placement?.origin ?? "left top",
        }}
        className="cf-glass fixed max-w-[min(20rem,calc(100vw-16px))] min-w-[200px] rounded-[14px] p-1.5 text-(--cf-label) outline-none select-none"
      >
        {menu.items.map((item, index) =>
          "separator" in item ? (
            <div
              key={`separator-${index}`}
              role="separator"
              className="mx-2.5 my-1 h-px bg-(--cf-separator)"
            />
          ) : (
            <div
              key={item.id}
              id={`context-menu-item-${menu.key}-${index}`}
              role="menuitem"
              aria-disabled={item.disabled || undefined}
              data-active={index === active || undefined}
              onPointerMove={() => {
                if (!item.disabled && active !== index) setActive(index);
              }}
              onClick={() => run(item)}
              className={cn([
                "flex h-7 items-center gap-6 rounded-[8px] px-2.5 text-[13px] leading-none",
                item.disabled
                  ? "text-(--cf-tertiary)"
                  : isDestructive(item)
                    ? "text-(--cf-record)"
                    : "text-(--cf-label)",
                index === active && "cf-glass-sel",
              ])}
            >
              <span className="min-w-0 flex-1 truncate">{item.text}</span>
              {item.accelerator ? (
                <span className="shrink-0 text-[12px] tracking-wide text-(--cf-tertiary)">
                  {formatAccelerator(item.accelerator)}
                </span>
              ) : null}
            </div>
          ),
        )}
      </motion.div>
    </div>
  );
}
