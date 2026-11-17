import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { generateText, streamText } from "ai";
import { describe, expect, it, vi } from "vitest";

import { AiEngineError, isAiEngineError } from "./errors";
import { createEngineModel, ENGINE_MODEL_ID } from "./model";
import { stubTransport, textAnswer } from "./testing";
import type { EngineTransport } from "./transport";

vi.mock("@anlg/plugin-store2", () => ({
  commands: {
    getStr: async () => ({ status: "ok", data: null }),
    setStr: async () => ({ status: "ok", data: null }),
    save: async () => ({ status: "ok", data: null }),
  },
}));

describe("Efficiency Engine client", () => {
  it("pins the same model id as the Rust Engine", () => {
    const rust = readFileSync(
      resolve(__dirname, "../../../../../crates/ai-engine/src/config.rs"),
      "utf8",
    );
    expect(rust).toContain(`pub const HAIKU_MODEL: &str = "${ENGINE_MODEL_ID}";`);
  });

  it("sends every call to the Engine as a Messages request, never to a host", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const { transport, requests } = stubTransport(() => textAnswer("Hello"));
    const model = createEngineModel({ task: "chat", transport });

    const result = streamText({
      model,
      prompt: "hi",
      temperature: 0,
    });
    expect(await result.text).toBe("Hello");

    expect(requests).toHaveLength(1);
    expect(requests[0]!.path).toBe("/v1/messages");
    const body = JSON.parse(requests[0]!.body);
    expect(body.stream).toBe(true);
    expect(fetchSpy).not.toHaveBeenCalled();
    // The placeholder key and SDK headers never cross into the request.
    expect(JSON.stringify(requests[0])).not.toContain("engine-managed");
    fetchSpy.mockRestore();
  });

  it("turns a pause into one calm error and does not retry", async () => {
    const { transport, requests } = stubTransport(() => ({
      code: "ai.quota",
      message: "today's AI call limit is reached",
    }));
    const model = createEngineModel({ transport });

    const error = await generateText({
      model,
      prompt: "hi",
      maxRetries: 4,
    }).catch((e: unknown) => e);

    expect(requests).toHaveLength(1);
    expect(isAiEngineError(error)).toBe(true);
    expect((error as AiEngineError).code).toBe("ai.quota");
    expect((error as Error).message).toMatch(/^AI paused: /);
    expect((error as Error).message).toContain(
      "Recording, transcripts and notes keep working",
    );
  });

  it("cancels the Engine request when the caller aborts", async () => {
    let release: () => void = () => undefined;
    const cancel = vi.fn(() => release());
    const transport: EngineTransport = {
      send: (_request, onEvent) =>
        new Promise((resolveSend) => {
          onEvent({ type: "head", status: 200, contentType: "text/event-stream" });
          release = () =>
            resolveSend({
              status: "error",
              error: { code: "ai.cancelled", message: "cancelled" },
            });
        }),
      cancel,
    };
    const controller = new AbortController();
    const result = streamText({
      model: createEngineModel({ transport }),
      prompt: "hi",
      abortSignal: controller.signal,
    });
    const reading = Promise.resolve(result.text).catch(() => "aborted");
    await new Promise((r) => setTimeout(r, 10));
    controller.abort();
    await reading;
    expect(cancel).toHaveBeenCalledTimes(1);
  });
});
