/**
 * Contract between the Rust backend and the React frontend.
 *
 * The frontend speaks the canonical AG-UI message shape everywhere: to the
 * model transport, and to thread storage. Rust converts to Chat Completions
 * when it calls the provider and back again when it streams. Keeping one shape
 * means `identityMessageFormat` is correct for both directions.
 */

export interface ToolCall {
  id: string;
  type: "function";
  function: { name: string; arguments: string };
}

/** One entry of the conversation history, as AG-UI defines it. */
export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "tool" | "system";
  content?: string | Array<{ type: "text"; text: string }>;
  toolCalls?: ToolCall[];
  toolCallId?: string;
  error?: string;
}

/** Events Rust emits, mirroring the subset of AG-UI that the chat consumes. */
export type AgUiEvent =
  | { type: "TEXT_MESSAGE_START"; messageId: string; role: "assistant" }
  | { type: "TEXT_MESSAGE_CONTENT"; messageId: string; delta: string }
  | { type: "TEXT_MESSAGE_END"; messageId: string }
  | { type: "TOOL_CALL_START"; toolCallId: string; toolCallName: string }
  | { type: "TOOL_CALL_ARGS"; toolCallId: string; delta: string }
  | { type: "TOOL_CALL_END"; toolCallId: string }
  | {
      type: "TOOL_CALL_RESULT";
      messageId: string;
      toolCallId: string;
      content: string;
      role: "tool";
      isError?: boolean;
      error?: string;
    }
  | { type: "RUN_ERROR"; message: string; code?: string };

/** Frames sent over the streaming channel, in order, ending with `done` or `error`. */
export type ChatFrame =
  | { kind: "event"; event: AgUiEvent }
  | { kind: "error"; message: string }
  | { kind: "done" };

/** A stored thread row, matching the headless `Thread` shape. */
export interface StoredThread {
  id: string;
  title: string;
  createdAt: string;
}

export interface ThreadList {
  threads: StoredThread[];
  nextCursor?: string;
}

/** Persisted user settings. The API key never travels back to the frontend. */
export interface SettingsSnapshot {
  baseUrl: string;
  model: string;
  apiKeySet: boolean;
}

export interface SettingsPatch {
  baseUrl?: string;
  model?: string;
  apiKey?: string;
}
