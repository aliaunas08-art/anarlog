import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import type { MouseEvent } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ContextMenuHost, formatAccelerator } from "./context-menu-host";

import {
  closeContextMenu,
  type MenuItemDef,
  openContextMenu,
} from "~/shared/hooks/useNativeContextMenu";

function open(items: MenuItemDef[], x = 40, y = 60) {
  const event = {
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
    clientX: x,
    clientY: y,
    currentTarget: document.body,
  } as unknown as MouseEvent;
  act(() => openContextMenu(items, event));
  return event;
}

describe("ContextMenuHost", () => {
  afterEach(() => {
    act(() => closeContextMenu());
    cleanup();
  });

  it("renders items, runs an action and closes", () => {
    const show = vi.fn();
    const remove = vi.fn();
    render(<ContextMenuHost />);
    const event = open([
      { id: "show", text: "Show in Finder", action: show },
      { separator: true },
      { id: "delete", text: "Delete Note", action: remove },
    ]);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(screen.getByRole("menu")).toBeTruthy();
    expect(screen.getAllByRole("menuitem")).toHaveLength(2);
    expect(screen.getByRole("separator")).toBeTruthy();
    expect(screen.getByText("Delete Note").parentElement?.className).toContain(
      "text-(--cf-record)",
    );

    fireEvent.click(screen.getByText("Show in Finder"));
    expect(show).toHaveBeenCalledTimes(1);
    expect(remove).not.toHaveBeenCalled();
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("moves with the arrow keys past disabled items and activates with Enter", () => {
    const first = vi.fn();
    const third = vi.fn();
    render(<ContextMenuHost />);
    open([
      { id: "a", text: "First", action: first },
      { id: "b", text: "Second", action: vi.fn(), disabled: true },
      { id: "c", text: "Third", action: third },
    ]);
    const menu = screen.getByRole("menu");
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    fireEvent.keyDown(menu, { key: "Enter" });
    expect(third).toHaveBeenCalledTimes(1);
    expect(first).not.toHaveBeenCalled();
  });

  it("does not run disabled items and closes on Escape or an outside press", () => {
    const disabled = vi.fn();
    render(<ContextMenuHost />);
    open([{ id: "a", text: "Nope", action: disabled, disabled: true }]);
    fireEvent.click(screen.getByText("Nope"));
    expect(disabled).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();

    open([{ id: "a", text: "Item", action: vi.fn() }]);
    const layer = screen.getByRole("menu").parentElement!;
    fireEvent.pointerDown(layer);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("formats accelerators as macOS glyphs", () => {
    expect(formatAccelerator("Backspace")).toBe("⌫");
    expect(formatAccelerator("CmdOrCtrl+Shift+n")).toBe("⌘⇧N");
  });
});
