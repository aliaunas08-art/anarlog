import { expect, test } from "vitest";

import { AiEngineError, pauseMessage } from "./errors";

const MARKER = "zqxcontentmarker4417";

// ASPC C4.2: pause messages are fixed text; a provider detail is shown only
// for a quota (Rust's own wording). Rust drops the provider's message on
// other errors (`provider_errors_never_carry_meeting_content`).
test("pause messages never echo provider detail that could quote a meeting", () => {
  for (const code of [
    "ai.unavailable",
    "ai.no_key",
    "ai.rate_limited",
    "ai.invalid_key",
  ]) {
    expect(pauseMessage(code, `echo ${MARKER}`)).not.toContain(MARKER);
    expect(
      new AiEngineError({ code, message: `echo ${MARKER}` }).message,
    ).not.toContain(MARKER);
  }
});
