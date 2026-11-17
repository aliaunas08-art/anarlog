import { vi } from "vitest";

import type { EngineEvent, EngineRequest } from "@anlg/plugin-ai-engine";

import { ENGINE_MODEL_ID } from "./model";
import type { EngineTransport } from "./transport";

// Test helpers: a stub Engine transport that records requests. Never used
// by app code.

export const sse = (text: string) =>
  [
    `event: message_start\ndata: ${JSON.stringify({
      type: "message_start",
      message: {
        id: "msg_1",
        type: "message",
        role: "assistant",
        model: ENGINE_MODEL_ID,
        content: [],
        stop_reason: null,
        usage: { input_tokens: 10, output_tokens: 1 },
      },
    })}\n\n`,
    `event: content_block_start\ndata: ${JSON.stringify({
      type: "content_block_start",
      index: 0,
      content_block: { type: "text", text: "" },
    })}\n\n`,
    `event: content_block_delta\ndata: ${JSON.stringify({
      type: "content_block_delta",
      index: 0,
      delta: { type: "text_delta", text },
    })}\n\n`,
    `event: content_block_stop\ndata: {"type":"content_block_stop","index":0}\n\n`,
    `event: message_delta\ndata: ${JSON.stringify({
      type: "message_delta",
      delta: { stop_reason: "end_turn" },
      usage: { output_tokens: 3 },
    })}\n\n`,
    `event: message_stop\ndata: {"type":"message_stop"}\n\n`,
  ].join("");

/** A stub Engine: records requests and answers with canned events. */
export function stubTransport(
  respond: (
    request: EngineRequest,
  ) => EngineEvent[] | { code: string; message: string },
) {
  const requests: EngineRequest[] = [];
  const transport: EngineTransport = {
    send: async (request, onEvent) => {
      requests.push(request);
      const answer = respond(request);
      if (!Array.isArray(answer)) return { status: "error", error: answer };
      for (const event of answer) onEvent(event);
      return { status: "ok" };
    },
    cancel: vi.fn(),
  };
  return { transport, requests };
}

export function textAnswer(text: string): EngineEvent[] {
  return [
    { type: "head", status: 200, contentType: "text/event-stream" },
    { type: "chunk", data: sse(text) },
    { type: "end" },
  ];
}

