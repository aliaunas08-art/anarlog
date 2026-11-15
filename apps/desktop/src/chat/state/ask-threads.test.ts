import { beforeEach, describe, expect, it } from "vitest";

import { serializeAskThreads } from "./ask-thread-persistence";
import {
  effectiveAskPreference,
  freshAskSessionId,
  selectChatSelection,
  useChatContext,
} from "./chat-context";

import { askThreadKey, resolveAskScope } from "~/chat/guardrails/scope";

function effectiveScope(openMeetingId: string | undefined) {
  return resolveAskScope(
    effectiveAskPreference(useChatContext.getState(), openMeetingId),
    openMeetingId,
  );
}

describe("Ask thread state", () => {
  beforeEach(() => {
    useChatContext.setState({
      askThreads: {},
      askScopePreference: null,
      askScopePreferenceMeetingId: null,
      pendingAskResend: null,
    });
  });

  it("#5 a scope choice lasts only while the same meeting is open", () => {
    useChatContext.getState().setAskScope("library", "m1");
    expect(effectiveScope("m1")).toBe("library");
    // Another meeting opens: back to the default, "This meeting".
    expect(effectiveScope("m2")).toBe("meeting");
    // Home (no meeting): default "All meetings".
    expect(effectiveScope(undefined)).toBe("library");
  });

  it("#5 an 'All meetings' re-ask is one-off: it does not follow the user to other meetings", () => {
    useChatContext
      .getState()
      .requestAskResend("library", "What did Sam promise?", "m1");
    expect(useChatContext.getState().pendingAskResend).toMatchObject({
      scope: "library",
      text: "What did Sam promise?",
    });
    expect(effectiveScope("m1")).toBe("library");
    expect(effectiveScope("m2")).toBe("meeting");
  });

  it("#5 prepareAskThread selects the meeting thread and returns its session id for the queued prompt", () => {
    useChatContext.getState().setAskScope("library", "m1");
    const sessionId = useChatContext
      .getState()
      .prepareAskThread("meeting", "m1");
    expect(effectiveScope("m1")).toBe("meeting");
    expect(sessionId).toBe(freshAskSessionId(askThreadKey("meeting", "m1")));

    const homeSession = useChatContext
      .getState()
      .prepareAskThread("library", undefined);
    expect(homeSession).toBe(freshAskSessionId("library"));
    expect(effectiveScope(undefined)).toBe("library");
  });

  it("#5/#11 switching meeting gives a different thread selection", () => {
    useChatContext.getState().setGroupId("general", "group-m1", "meeting:m1");
    const m1 = selectChatSelection(
      useChatContext.getState(),
      "general",
      "meeting:m1",
    );
    const m2 = selectChatSelection(
      useChatContext.getState(),
      "general",
      "meeting:m2",
    );
    expect(m1.groupId).toBe("group-m1");
    expect(m2.groupId).toBeUndefined();
    expect(m2.sessionId).not.toBe(m1.sessionId);
  });

  it("#7 a previous (pre-scope) chat opens read-only in All meetings without losing that thread", () => {
    useChatContext.getState().setGroupId("general", "lib-group", "library");
    useChatContext.getState().openPreviousChat("old-group", "m1");
    expect(effectiveScope("m1")).toBe("library");
    expect(
      selectChatSelection(useChatContext.getState(), "general", "library"),
    ).toMatchObject({ groupId: "old-group", sessionId: "old-group", legacy: true });
    // The All meetings thread it covers for is still what gets saved.
    expect(
      JSON.parse(serializeAskThreads(useChatContext.getState().askThreads)),
    ).toEqual({
      library: { groupId: "lib-group", sessionId: freshAskSessionId("library") },
    });
  });

  it("#7 persisted threads keep keyed groups only (no fresh or read-only threads)", () => {
    const json = serializeAskThreads({
      "meeting:m1": { groupId: "g1", sessionId: "s1" },
      library: { groupId: undefined, sessionId: "s2" },
      old: { groupId: "old", sessionId: "old", legacy: true },
    });
    expect(JSON.parse(json)).toEqual({
      "meeting:m1": { groupId: "g1", sessionId: "s1" },
    });
  });

  it("a stored thread under the removed General key is never selected", () => {
    useChatContext.setState({
      askThreads: { general: { groupId: "g", sessionId: "g" } },
    });
    expect(effectiveScope(undefined)).toBe("library");
    expect(effectiveScope("m1")).toBe("meeting");
    expect(
      selectChatSelection(useChatContext.getState(), "general", "library")
        .groupId,
    ).toBeUndefined();
  });

  it("#9 a pending re-ask can be cleared by its id only", () => {
    useChatContext.getState().requestAskResend("library", "q", undefined);
    const pending = useChatContext.getState().pendingAskResend!;
    useChatContext.getState().clearAskResend("other-id");
    expect(useChatContext.getState().pendingAskResend).not.toBeNull();
    useChatContext.getState().clearAskResend(pending.requestId);
    expect(useChatContext.getState().pendingAskResend).toBeNull();
  });
});
