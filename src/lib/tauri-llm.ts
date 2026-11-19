import { Channel, invoke } from "@tauri-apps/api/core";
import { agUIAdapter, type ChatLLM } from "@openuidev/react-headless";
import type { ChatFrame, ChatMessage } from "./contract";

/**
 * Resolves once the run has finished, failed, or been cancelled. The chat store
 * awaits `send` only for headers, so the real work happens inside the body.
 */
function startRun(runId: string, threadId: string, messages: ChatMessage[]) {
  const channel = new Channel<ChatFrame>();
  let settled = false;

  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      const encoder = new TextEncoder();

      const push = (event: unknown) => {
        controller.enqueue(encoder.encode(`data: ${JSON.stringify(event)}\n\n`));
      };

      const finish = () => {
        if (settled) return;
        settled = true;
        controller.close();
      };

      const fail = (message: string) => {
        if (settled) return;
        // The adapter turns this into the thread-level error the UI displays.
        push({ type: "RUN_ERROR", message });
        finish();
      };

      channel.onmessage = (frame) => {
        switch (frame.kind) {
          case "event":
            push(frame.event);
            break;
          case "error":
            fail(frame.message);
            break;
          case "done":
            finish();
            break;
        }
      };

      invoke("chat", { request: { runId, threadId, messages }, onEvent: channel }).catch(
        (error: unknown) => fail(error instanceof Error ? error.message : String(error)),
      );
    },
    cancel() {
      settled = true;
      void invoke("cancel_chat", { runId });
    },
  });

  return new Response(stream, {
    status: 200,
    headers: { "Content-Type": "text/event-stream", "Cache-Control": "no-cache" },
  });
}

/**
 * Bridges the stock chat store to the Rust agent. Rust emits the same AG-UI
 * events the template's LangGraph route emitted, so the renderer is unchanged.
 */
export const tauriLLM: ChatLLM = {
  streamProtocol: agUIAdapter(),
  send: ({ threadId, messages, signal }) => {
    const runId = crypto.randomUUID();
    signal.addEventListener("abort", () => {
      void invoke("cancel_chat", { runId });
    });
    return Promise.resolve(startRun(runId, threadId, messages as ChatMessage[]));
  },
};
