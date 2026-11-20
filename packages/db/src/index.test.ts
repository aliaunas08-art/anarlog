import { getTableColumns } from "drizzle-orm";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createDb, events, max, sessions, templates } from "./index";

describe("@anlg/db createDb", () => {
  const executeProxy = vi.fn();
  const db = createDb({ executeProxy });

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("uses executeProxy for inserts", async () => {
    executeProxy.mockResolvedValue({ rows: [] });

    await db.insert(templates).values({
      id: "template-1",
      title: "New Template",
      description: "",
      pinned: false,
      pinOrder: null,
      category: null,
      targetsJson: null,
      sectionsJson: [],
      createdAt: "2026-04-14T00:00:00Z",
      updatedAt: "2026-04-14T00:00:00Z",
    });

    expect(executeProxy).toHaveBeenCalledWith(
      expect.stringContaining('insert into "templates"'),
      expect.any(Array),
      "run",
    );
  });

  it("exposes nullable event attendance JSON through the Drizzle schema", () => {
    const attendanceJson = getTableColumns(events).attendanceJson;

    expect(attendanceJson.name).toBe("attendance_json");
    expect(attendanceJson.notNull).toBe(false);
    expect(attendanceJson.dataType).toBe("json");
  });

  it("serializes event attendance JSON through the proxy", async () => {
    executeProxy.mockResolvedValue({ rows: [] });

    await db.insert(events).values({
      id: "event-1",
      attendanceJson: {
        version: 1,
        self_status: "accepted",
      },
      createdAt: "2026-09-30T10:00:00Z",
      updatedAt: "2026-09-30T10:00:00Z",
    });

    expect(executeProxy).toHaveBeenCalledWith(
      expect.stringContaining('"attendance_json"'),
      expect.arrayContaining([
        JSON.stringify({ version: 1, self_status: "accepted" }),
      ]),
      "run",
    );
  });

  it("maps proxy rows for findMany", async () => {
    executeProxy.mockResolvedValue({
      rows: [
        [
          "template-1",
          "One",
          "",
          0,
          null,
          null,
          "{}",
          null,
          "[]",
          "2026-04-14T00:00:00Z",
          "2026-04-14T00:00:00Z",
        ],
      ],
    });

    await expect(db.select().from(templates)).resolves.toEqual([
      {
        id: "template-1",
        title: "One",
        description: "",
        pinned: false,
        pinOrder: null,
        category: null,
        iconJson: {},
        targetsJson: null,
        sectionsJson: [],
        createdAt: "2026-04-14T00:00:00Z",
        updatedAt: "2026-04-14T00:00:00Z",
      },
    ]);
  });

  it("uses get mode for findFirst", async () => {
    executeProxy.mockResolvedValue({
      rows: [
        "template-1",
        "One",
        "",
        0,
        null,
        null,
        "{}",
        null,
        "[]",
        "2026-04-14T00:00:00Z",
        "2026-04-14T00:00:00Z",
      ],
    });

    await expect(db.query.templates.findFirst()).resolves.toEqual({
      id: "template-1",
      title: "One",
      description: "",
      pinned: false,
      pinOrder: null,
      category: null,
      iconJson: {},
      targetsJson: null,
      sectionsJson: [],
      createdAt: "2026-04-14T00:00:00Z",
      updatedAt: "2026-04-14T00:00:00Z",
    });
  });

  it("passes all mode through to the proxy client", async () => {
    executeProxy.mockResolvedValue({ rows: [] });

    await db.select().from(templates);

    expect(executeProxy).toHaveBeenCalledWith(
      expect.stringContaining('select "id", "title"'),
      expect.any(Array),
      "all",
    );
  });

  it("maps aggregate query rows from the proxy client", async () => {
    executeProxy.mockResolvedValue({ rows: [[7]] });

    await expect(
      db.select({ maxOrder: max(templates.pinOrder) }).from(templates),
    ).resolves.toEqual([{ maxOrder: 7 }]);
  });

  it("rethrows proxy errors", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const error = new Error("proxy failed");
    executeProxy.mockRejectedValue(error);

    await expect(db.select().from(templates)).rejects.toThrow(/Failed query:/);
    errorSpy.mockRestore();
  });

  it("fills workspace and owner ids from the local identity, never '' (ASPC C1.1)", async () => {
    executeProxy.mockResolvedValue({ rows: [] });

    await db.insert(sessions).values({ id: "session-1", title: "Planning" });

    const [query, params] = executeProxy.mock.calls[0];
    expect(query).toContain('"workspace_id"');
    expect(query).toContain('"owner_user_id"');
    expect(query).toContain("cloudsync_workspace_binding");
    expect(query).toContain("local_library_connections");
    // id and title are the only bound values ahead of the other defaults.
    expect(params.slice(0, 2)).toEqual(["session-1", "Planning"]);
  });

  it("keeps an explicit workspace id", async () => {
    executeProxy.mockResolvedValue({ rows: [] });

    await db.insert(sessions).values({
      id: "session-2",
      workspaceId: "workspace-1",
      ownerUserId: "owner-1",
    });

    const [query, params] = executeProxy.mock.calls[0];
    expect(query).not.toContain("cloudsync_workspace_binding");
    expect(params).toEqual(expect.arrayContaining(["workspace-1", "owner-1"]));
  });
});
