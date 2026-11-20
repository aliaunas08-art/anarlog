import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@anlg/ui/components/ui/dropdown-menu";

const mocks = vi.hoisted(() => ({
  setSessionPrivate: vi.fn(),
  session: { private: false } as { private: boolean } | null,
}));

vi.mock("@anlg/plugin-session", () => ({
  commands: { setSessionPrivate: mocks.setSessionPrivate },
}));

vi.mock("~/session/queries", () => ({
  useSession: () => mocks.session,
}));

import { PrivateNote } from "./private-note";

function renderInMenu() {
  return render(
    <DropdownMenu open>
      <DropdownMenuTrigger>menu</DropdownMenuTrigger>
      <DropdownMenuContent>
        <PrivateNote sessionId="meeting-1" />
      </DropdownMenuContent>
    </DropdownMenu>,
  );
}

describe("PrivateNote", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.setSessionPrivate.mockResolvedValue({ status: "ok", data: true });
  });

  afterEach(() => {
    cleanup();
  });

  it("asks Rust to set the flag, so the change is audit-logged (D1.4)", () => {
    mocks.session = { private: false };
    renderInMenu();
    fireEvent.click(screen.getByText("Make Private"));
    expect(mocks.setSessionPrivate).toHaveBeenCalledWith("meeting-1", true);
  });

  it("clears it the same way", () => {
    mocks.session = { private: true };
    renderInMenu();
    fireEvent.click(screen.getByText("Make Not Private"));
    expect(mocks.setSessionPrivate).toHaveBeenCalledWith("meeting-1", false);
  });
});
