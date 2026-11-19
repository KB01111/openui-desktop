import { useEffect, useId, useState, type FormEvent } from "react";
import { Button, Input, useNav } from "@openuidev/react-ui";
import type { SettingsPatch } from "../lib/contract";
import { describeError, type SettingsController } from "../lib/use-settings";

type Status = { tone: "ok" | "error"; text: string };

/** Rendered by <AgentInterface.Route path="settings"> in place of the thread. */
export function SettingsPage({ settings }: { settings: SettingsController }) {
  const { navigate } = useNav();
  const { snapshot, loadError, save } = settings;

  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [seeded, setSeeded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState<Status | null>(null);

  const apiKeyId = useId();
  const modelId = useId();
  const baseUrlId = useId();

  useEffect(() => {
    if (seeded || !snapshot) return;
    setModel(snapshot.model);
    setBaseUrl(snapshot.baseUrl);
    setSeeded(true);
  }, [seeded, snapshot]);

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (saving) return;

    const patch: SettingsPatch = {};
    const nextModel = model.trim();
    const nextBaseUrl = baseUrl.trim();
    const key = apiKey.trim();
    if (nextModel) patch.model = nextModel;
    if (nextBaseUrl) patch.baseUrl = nextBaseUrl;
    if (key) patch.apiKey = key;

    if (!patch.model && !patch.baseUrl && !patch.apiKey) {
      setStatus({ tone: "error", text: "Nothing to save yet." });
      return;
    }

    setSaving(true);
    setStatus(null);
    try {
      await save(patch);
      setApiKey("");
      setStatus({ tone: "ok", text: "Saved. New messages use these settings." });
    } catch (cause) {
      setStatus({ tone: "error", text: `Could not save: ${describeError(cause)}` });
    } finally {
      setSaving(false);
    }
  };

  const keySet = snapshot?.apiKeySet === true;
  const keyBadge = snapshot
    ? keySet
      ? "API key configured"
      : "API key not configured"
    : loadError
      ? "API key status unknown"
      : "Checking API key…";

  return (
    <div className="openui-desktop-settings">
      <div className="openui-desktop-settings__panel">
        <div className="openui-desktop-settings__header">
          <div>
            <h1 className="openui-desktop-settings__title">Settings</h1>
            <p className="openui-desktop-settings__subtitle">
              OpenUI Desktop talks to any OpenAI-compatible endpoint. The key is stored by the app, never sent to
              this window.
            </p>
          </div>
          <Button variant="tertiary" size="small" onClick={() => navigate(undefined)}>
            Back to chat
          </Button>
        </div>

        <span
          className={`openui-desktop-settings__badge ${snapshot ? (keySet ? "is-set" : "is-unset") : ""}`}
          role="status"
          aria-live="polite"
        >
          <span className="openui-desktop-settings__dot" aria-hidden="true" />
          {keyBadge}
        </span>

        {loadError ? (
          <p className="openui-desktop-settings__notice is-error">
            Could not read settings from the app: {loadError}
          </p>
        ) : null}

        {!keySet ? (
          <p className="openui-desktop-settings__notice">
            Add an API key below to start chatting. Until then every message fails with a prompt to open Settings.
          </p>
        ) : null}

        <form className="openui-desktop-settings__card" onSubmit={handleSubmit}>
          <div className="openui-desktop-settings__field">
            <label className="openui-desktop-settings__label" htmlFor={apiKeyId}>
              API key
            </label>
            <Input
              id={apiKeyId}
              type="password"
              placeholder="sk-…"
              autoComplete="off"
              spellCheck={false}
              value={apiKey}
              onChange={(event) => setApiKey(event.target.value)}
            />
            <span className="openui-desktop-settings__hint">
              {keySet ? "A key is already saved. Leave empty to keep it." : "Required before the first message."}
            </span>
          </div>

          <div className="openui-desktop-settings__field">
            <label className="openui-desktop-settings__label" htmlFor={modelId}>
              Model
            </label>
            <Input
              id={modelId}
              type="text"
              placeholder="gpt-4o-mini"
              autoComplete="off"
              spellCheck={false}
              value={model}
              onChange={(event) => setModel(event.target.value)}
            />
          </div>

          <div className="openui-desktop-settings__field">
            <label className="openui-desktop-settings__label" htmlFor={baseUrlId}>
              Base URL
            </label>
            <Input
              id={baseUrlId}
              type="text"
              placeholder="https://api.openai.com/v1"
              autoComplete="off"
              spellCheck={false}
              value={baseUrl}
              onChange={(event) => setBaseUrl(event.target.value)}
            />
            <span className="openui-desktop-settings__hint">Point this at a compatible server or a local proxy.</span>
          </div>

          <div className="openui-desktop-settings__actions">
            <Button type="submit" variant="primary" size="small" disabled={saving}>
              {saving ? "Saving…" : "Save"}
            </Button>
            {status ? (
              <p
                className={`openui-desktop-settings__status ${status.tone === "ok" ? "is-ok" : "is-error"}`}
                role="status"
                aria-live="polite"
              >
                {status.text}
              </p>
            ) : null}
          </div>
        </form>
      </div>
    </div>
  );
}
