import { Channel } from "@tauri-apps/api/core";

import {
  commands as engineCommands,
  type EngineError,
  type EngineEvent,
  type EngineRequest,
} from "@anlg/plugin-ai-engine";

import { AiEngineError } from "./errors";

/**
 * Sends one request to the Rust Engine. Resolves when the Engine finished
 * (ok) or failed (error); events arrive through `onEvent` meanwhile.
 */
export type EngineSend = (
  request: EngineRequest,
  onEvent: (event: EngineEvent) => void,
) => Promise<{ status: "ok" } | { status: "error"; error: EngineError }>;

export type EngineTransport = {
  send: EngineSend;
  cancel: (requestId: string) => void | Promise<void>;
};

export const tauriEngineTransport: EngineTransport = {
  send: async (request, onEvent) => {
    const channel = new Channel<EngineEvent>();
    channel.onmessage = onEvent;
    const result = await engineCommands.request(request, channel);
    return result.status === "ok" ? { status: "ok" } : result;
  },
  cancel: async (requestId) => {
    try {
      await engineCommands.cancelRequest(requestId);
    } catch {
      // The request may already have finished.
    }
  },
};

function abortError(): DOMException {
  return new DOMException("The operation was aborted.", "AbortError");
}

function requestPath(input: RequestInfo | URL): string {
  const href =
    typeof input === "string"
      ? input
      : input instanceof URL
        ? input.href
        : input.url;
  return new URL(href).pathname;
}

async function requestBody(body: BodyInit | null | undefined) {
  if (body == null) return "";
  if (typeof body === "string") return body;
  return new Response(body).text();
}

/**
 * A `fetch` for the AI SDK that hands the request to the Rust Engine.
 * Only the path, the JSON body and `anthropic-beta` are passed on; the
 * Engine picks the host, the auth header, the API version and the model.
 * Headers the SDK sets (including the placeholder key) never leave JS.
 */
export function createEngineFetch(
  transport: EngineTransport = tauriEngineTransport,
): typeof fetch {
  return async (input, init) => {
    const signal = init?.signal ?? undefined;
    if (signal?.aborted) throw abortError();

    const beta = new Headers(init?.headers).get("anthropic-beta");
    const request: EngineRequest = {
      request_id: crypto.randomUUID(),
      path: requestPath(input),
      body: await requestBody(init?.body),
      betas: beta ? [beta] : [],
    };

    return new Promise<Response>((resolve, reject) => {
      const encoder = new TextEncoder();
      let controller!: ReadableStreamDefaultController<Uint8Array>;
      let headSent = false;
      let done = false;
      const body = new ReadableStream<Uint8Array>({
        start(c) {
          controller = c;
        },
        cancel() {
          if (!done) void transport.cancel(request.request_id);
        },
      });

      const onAbort = () => {
        if (done) return;
        void transport.cancel(request.request_id);
        fail(abortError());
      };
      signal?.addEventListener("abort", onAbort, { once: true });

      const finish = () => {
        done = true;
        signal?.removeEventListener("abort", onAbort);
      };
      const fail = (error: unknown) => {
        if (done) return;
        finish();
        if (headSent) controller.error(error);
        else reject(error);
      };

      transport
        .send(request, (event) => {
          if (done) return;
          if (event.type === "head") {
            headSent = true;
            resolve(
              new Response(body, {
                status: event.status,
                headers: { "content-type": event.contentType },
              }),
            );
          } else if (event.type === "chunk") {
            controller.enqueue(encoder.encode(event.data));
          } else if (event.type === "end") {
            finish();
            controller.close();
          }
        })
        .then(
          (result) => {
            if (result.status === "error") {
              fail(
                signal?.aborted || result.error.code === "ai.cancelled"
                  ? abortError()
                  : new AiEngineError(result.error),
              );
            }
          },
          (error: unknown) =>
            fail(
              new AiEngineError({
                code: "ai.unavailable",
                message: error instanceof Error ? error.message : "",
              }),
            ),
        );
    });
  };
}
