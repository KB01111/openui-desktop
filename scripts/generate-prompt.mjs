/**
 * Builds the OpenUI system prompt from the generated library spec.
 *
 * The prompt is a build artifact: the template shipped it by calling
 * `generateSystemPrompt` at request time on the server. Here the server is Rust,
 * so the prompt is rendered once during the frontend build and embedded into
 * the binary with `include_str!`. That keeps `@openuidev/lang-core` out of the
 * Rust dependency tree and guarantees the prompt matches the spec the browser
 * renders with.
 */
import { generateSystemPrompt } from "@openuidev/lang-core";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const specPath = resolve(root, "src/generated/spec.json");
const outPath = resolve(root, "src-tauri/prompt.txt");

const { promptOptions, ...library } = JSON.parse(readFileSync(specPath, "utf8"));
const prompt = generateSystemPrompt({ library, promptOptions });

mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, prompt, "utf8");

console.log(`wrote ${outPath} (${prompt.length} chars)`);
