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
    useChatContext.getState().setAskScope("general", "m1");
    expect(effectiveScope("m1")).toBe("general");
    // Another meeting opens: back to the default, "This meeting".
    expect(effectiveScope("m2")).toBe("meeting");
    // Home (no meeting): default "All meetings".
    expect(effectiveScope(undefined)).toBe("library");
  });

  it("#5 an 'Ask in General' re-ask is one-off: it does not follow the user to other meetings", () => {
    useChatContext
      .getState()
      .requestAskResend("general", "What is an orange", "m1");
    expect(useChatContext.getState().pendingAskResend).toMatchObject({
      scope: "general",
      text: "What is an orange",
    });
    expect(effectiveScope("m1")).toBe("general");
    expect(effectiveScope("m2")).toBe("meeting");
  });

  it("#5 prepareAskThread selects the meeting thread and returns its session id for the queued prompt", () => {
    useChatContext.getState().setAskScope("general", "m1");
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

  it("#7 a previous (pre-scope) chat opens read-only in General", () => {
    useChatContext.getState().openPreviousChat("old-group", "m1");
    expect(effectiveScope("m1")).toBe("general");
    expect(
      selectChatSelection(useChatContext.getState(), "general", "general"),
    ).toEqual({ groupId: "old-group", sessionId: "old-group", legacy: true });
  });

  it("#7 persisted threads keep keyed groups only (no fresh or read-only threads)", () => {
    const json = serializeAskThreads({
      "meeting:m1": { groupId: "g1", sessionId: "s1" },
      library: { groupId: undefined, sessionId: "s2" },
      general: { groupId: "old", sessionId: "old", legacy: true },
    });
    expect(JSON.parse(json)).toEqual({
      "meeting:m1": { groupId: "g1", sessionId: "s1" },
    });
  });

  it("#9 a pending re-ask can be cleared by its id only", () => {
    useChatContext.getState().requestAskResend("general", "q", undefined);
    const pending = useChatContext.getState().pendingAskResend!;
    useChatContext.getState().clearAskResend("other-id");
    expect(useChatContext.getState().pendingAskResend).not.toBeNull();
    useChatContext.getState().clearAskResend(pending.requestId);
    expect(useChatContext.getState().pendingAskResend).toBeNull();
  });
});
