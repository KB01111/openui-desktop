import { invoke } from "@tauri-apps/api/core";
import type { ChatStorage, Message } from "@openuidev/react-headless";
import type {
  ChatMessage,
  SettingsPatch,
  SettingsSnapshot,
  StoredThread,
  ThreadList,
} from "./contract";

/**
 * Threads live in the app's data directory, written by Rust. The chat command
 * appends each completed turn, so `getMessages` returns the history that
 * actually reached the model, tool calls and results included.
 */
export const tauriStorage: ChatStorage = {
  thread: {
    listThreads: (cursor) => invoke<ThreadList>("list_threads", { cursor: cursor ?? null }),

    createThread: (firstMessage) =>
      invoke<StoredThread>("create_thread", { message: firstMessage as ChatMessage }),

    getMessages: async (threadId) =>
      (await invoke<ChatMessage[]>("get_messages", { threadId })) as Message[],

    updateThread: (thread) => invoke<StoredThread>("update_thread", { thread }),

    deleteThread: (id) => invoke<void>("delete_thread", { threadId: id }),
  },
};

export const getSettings = () => invoke<SettingsSnapshot>("get_settings");

export const saveSettings = (patch: SettingsPatch) =>
  invoke<SettingsSnapshot>("save_settings", { patch });
