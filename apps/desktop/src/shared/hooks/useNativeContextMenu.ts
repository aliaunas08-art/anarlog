import { type MouseEvent, useCallback } from "react";

/**
 * Right-click menus. The name is historical: these used to be native macOS
 * menus, which cannot take the app's glass styling. They now render in the
 * webview through `<ContextMenuHost />` (mounted once per window) with the
 * same item definitions.
 */
export type MenuItemDef =
  | {
      id: string;
      text: string;
      action: () => void;
      disabled?: boolean;
      accelerator?: string;
      /** Red text. Defaults to true for ids starting with "delete". */
      destructive?: boolean;
    }
  | { separator: true };

export type OpenContextMenu = Readonly<{
  key: number;
  items: readonly MenuItemDef[];
  x: number;
  y: number;
  returnFocus: HTMLElement | null;
}>;

let current: OpenContextMenu | null = null;
let nextKey = 0;
const listeners = new Set<() => void>();

function publish(next: OpenContextMenu | null) {
  current = next;
  listeners.forEach((listener) => listener());
}

export function subscribeContextMenu(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getContextMenu(): OpenContextMenu | null {
  return current;
}

export function closeContextMenu() {
  if (current) publish(null);
}

export function openContextMenu(items: readonly MenuItemDef[], e: MouseEvent) {
  e.preventDefault();
  e.stopPropagation();
  if (items.length === 0) return;

  let { clientX: x, clientY: y } = e;
  // A keyboard-invoked context menu (Shift+F10, menu key) has no pointer
  // position: anchor it to the element instead.
  if (x === 0 && y === 0 && e.currentTarget instanceof Element) {
    const rect = e.currentTarget.getBoundingClientRect();
    x = rect.left + 8;
    y = rect.bottom;
  }

  const active = document.activeElement;
  publish({
    key: ++nextKey,
    items,
    x,
    y,
    returnFocus: active instanceof HTMLElement ? active : null,
  });
}

export function useNativeContextMenu(items: MenuItemDef[]) {
  return useCallback(
    async (e: MouseEvent) => {
      openContextMenu(items, e);
    },
    [items],
  );
}
