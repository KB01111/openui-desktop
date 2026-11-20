# Helios AI

A native Windows desktop app for generative UI chat, ported from the
[OpenUI self-hosted template](https://github.com/thesysdev/openui/tree/main/templates/openui-self-hosted)
to [Tauri v2](https://tauri.app). The model writes OpenUI Lang, the app renders it
live as an interactive interface, and everything runs locally — no Next.js server,
no LangGraph runtime, no cloud account.

## What changed in the port

The template was a Next.js app with an in-process LangGraph agent. Both are gone;
the Rust backend replaces them.

| Template | This app |
| --- | --- |
| Next.js 16 + API route | Vite + one Tauri window |
| `src/agent.ts` (LangGraph agent) | `src-tauri/src/agent.rs` (native tool loop) |
| `/api/chat` streaming SSE | Tauri `Channel` → `ChatLLM` bridge |
| `generateSystemPrompt` at request time | prompt embedded at build time |
| In-memory threads | JSON threads in the app data directory |

The renderer stack is untouched: `@openuidev/react-ui`'s `AgentInterface`,
`openuiLibrary`, and the stock `agUIAdapter()` drive the UI. Rust emits the same
AG-UI events the template's LangGraph adapter produced, so the frontend needed no
rendering changes.

## Setup

Install dependencies, then run the app in development:

```bash
npm install
npm run desktop
```

On first launch, open **Settings** in the sidebar and add an API key. The key is
stored by the Rust side in the app data directory and is never sent back to the
web view. The base URL defaults to `https://api.openai.com/v1` and can point at
any OpenAI-compatible endpoint, including a local server.

As a fallback for development, the backend also reads `OPENAI_API_KEY` from the
environment.

## Build a Windows installer

```bash
npm run desktop:build
```

Outputs an NSIS installer at
`src-tauri/target/release/bundle/nsis/Helios AI_<version>_x64-setup.exe`,
installing per-user by default.

## How it fits together

```
React (Vite)                          Rust (Tauri)                    Provider
────────────                          ────────────                    ────────
AgentInterface
  └─ ChatLLM (agUIAdapter)
       └─ Channel<ChatFrame> ───────► chat command
                                       ├─ settings.json  (key, model, base URL)
                                       ├─ prompt.txt     (embedded system prompt)
                                       ├─ messages ─────► POST /chat/completions
                                       │                    SSE deltas ◄─┘
                                       ├─ get_weather tool (Open-Meteo, no key)
                                       ├─ AG-UI events ──►
                                       └─ append turn ───► threads/<id>.json
```

- `src/lib/contract.ts` is the single shared vocabulary: message shapes, AG-UI
  events, channel frames, settings.
- `src/lib/tauri-llm.ts` frames incoming frames as SSE so `agUIAdapter()` parses
  them unchanged, and forwards cancellation to `cancel_chat`.
- `src/lib/tauri-storage.ts` implements `ChatStorage` over Tauri commands, so
  threads and their tool calls survive restarts.

The system prompt is generated at build time by `scripts/generate-prompt.mjs`
from `src/generated/spec.json` and embedded with `include_str!`. Regenerating the
spec is a separate step; it needs the OpenUI CLI:

```bash
npx @openuidev/cli@latest generate src/lib/library.ts --spec --out src/generated/spec.json
```

## Tools

The agent runs tools natively, in Rust. `get_weather`
(`src-tauri/src/tools.rs`) needs no API key — ask "What's the weather in Berlin?"
to exercise the loop. Add tools by extending `tools::specs` and `tools::run`.

## Conversation storage

Threads are JSON documents under `%APPDATA%\com.openui.desktop\threads`. Each
completed turn is appended after the run succeeds, so a cancelled or failed run
never leaves a partial record. `create_thread` stores the first user message, and
only the genuinely new tail of the replayed history is appended afterwards, so
nothing is written twice.

To clear everything, delete that directory.

## Project layout

```
src/                     React frontend
  App.tsx                AgentInterface shell, slots, settings route
  lib/contract.ts        shared types with the backend
  lib/tauri-*.ts         LLM and storage bridges
  generated/spec.json    OpenUI library spec
src-tauri/
  src/agent.rs           the agent loop
  src/openai.rs          streaming Chat Completions client
  src/tools.rs           native tools
  src/store.rs           thread persistence
  src/settings.rs        settings persistence
  src/sse.rs             incremental SSE decoder
  prompt.txt             generated system prompt (embedded)
  tests/agent_loop.rs    end-to-end agent loop tests
```

## Checks

```bash
npm run typecheck          # frontend types
npm run build              # typecheck + production bundle
cd src-tauri
cargo test                 # 31 unit and integration tests
cargo clippy
```

## Learn more

- [OpenUI documentation](https://openui.com/docs)
- [OpenUI on GitHub](https://github.com/thesysdev/openui)
- [Tauri v2 documentation](https://tauri.app)
