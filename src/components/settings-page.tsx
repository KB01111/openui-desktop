import { useEffect, useId, useState, type FormEvent } from "react";
import { Button, Input, Slider, SwitchGroup, SwitchItem, TextArea, useNav } from "@openuidev/react-ui";
import type { SettingsPatch } from "../lib/contract";
import { describeError, type SettingsController } from "../lib/use-settings";

type Status = { tone: "ok" | "error"; text: string };

type ThemeModePreference = "light" | "dark" | "system";

const DEFAULT_TEMPERATURE = 0.7;

const PRESETS = [
  { label: "OpenAI", model: "gpt-4o-mini", baseUrl: "https://api.openai.com/v1" },
  { label: "Ollama", model: "llama3.1", baseUrl: "http://127.0.0.1:11434/v1" },
  { label: "LM Studio", model: "local-model", baseUrl: "http://127.0.0.1:1234/v1" },
] as const;

const THEME_OPTIONS: Array<{ value: ThemeModePreference; label: string }> = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

type ThemeProps = {
  preference: ThemeModePreference;
  setPreference: (next: ThemeModePreference) => void;
};

/** Rendered by <AgentInterface.Route path="settings"> in place of the thread. */
export function SettingsPage({ settings, theme }: { settings: SettingsController; theme: ThemeProps }) {
  const { navigate } = useNav();
  const { snapshot, loadError, save } = settings;

  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [temperature, setTemperature] = useState(DEFAULT_TEMPERATURE);
  const [maxTokens, setMaxTokens] = useState("");
  const [instructions, setInstructions] = useState("");
  const [weatherTool, setWeatherTool] = useState(true);
  const [seeded, setSeeded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState<Status | null>(null);

  const apiKeyId = useId();
  const modelId = useId();
  const baseUrlId = useId();
  const maxTokensId = useId();
  const instructionsId = useId();
  const connectionHeadingId = useId();
  const generationHeadingId = useId();
  const toolsHeadingId = useId();
  const appearanceHeadingId = useId();

  useEffect(() => {
    if (seeded || !snapshot) return;
    setModel(snapshot.model);
    setBaseUrl(snapshot.baseUrl);
    setTemperature(snapshot.temperature);
    setMaxTokens(snapshot.maxTokens == null ? "" : String(snapshot.maxTokens));
    setInstructions(snapshot.instructions ?? "");
    setWeatherTool(snapshot.weatherTool);
    setSeeded(true);
  }, [seeded, snapshot]);

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (saving) return;

    const patch: SettingsPatch = {
      temperature,
      instructions: instructions.trim(),
      weatherTool,
    };

    const nextModel = model.trim();
    const nextBaseUrl = baseUrl.trim();
    const key = apiKey.trim();
    if (nextModel) patch.model = nextModel;
    if (nextBaseUrl) patch.baseUrl = nextBaseUrl;
    if (key) patch.apiKey = key;

    const tokens = maxTokens.trim();
    if (tokens) {
      const parsed = Number(tokens);
      if (!Number.isFinite(parsed) || parsed <= 0) {
        setStatus({ tone: "error", text: "Max output tokens must be a positive number, or left empty." });
        return;
      }
      patch.maxTokens = Math.floor(parsed);
    } else {
      patch.maxTokens = 0;
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

  const handleRemoveKey = async () => {
    if (saving) return;
    setSaving(true);
    setStatus(null);
    try {
      await save({ removeApiKey: true });
      setApiKey("");
      setStatus({ tone: "ok", text: "API key removed." });
    } catch (cause) {
      setStatus({ tone: "error", text: `Could not remove the key: ${describeError(cause)}` });
    } finally {
      setSaving(false);
    }
  };

  const handleResetGeneration = () => {
    setTemperature(DEFAULT_TEMPERATURE);
    setMaxTokens("");
    setInstructions("");
    setStatus({ tone: "ok", text: "Generation fields reset. Save to apply." });
  };

  const handlePreset = (preset: (typeof PRESETS)[number]) => {
    setModel(preset.model);
    setBaseUrl(preset.baseUrl);
    setStatus({ tone: "ok", text: `${preset.label} preset applied. Save to apply.` });
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
    <div className="helios-settings">
      <div className="helios-settings__panel">
        <div className="helios-settings__header">
          <div>
            <h1 className="helios-settings__title">Settings</h1>
            <p className="helios-settings__subtitle">
              Helios AI talks to any OpenAI-compatible endpoint. Your key is stored by the app and never sent
              to this window.
            </p>
          </div>
          <Button variant="tertiary" size="small" onClick={() => navigate(undefined)}>
            Back to chat
          </Button>
        </div>

        <span
          className={`helios-settings__badge ${snapshot ? (keySet ? "is-set" : "is-unset") : ""}`}
          role="status"
          aria-live="polite"
        >
          <span className="helios-settings__dot" aria-hidden="true" />
          {keyBadge}
        </span>

        {loadError ? (
          <p className="helios-settings__notice is-error">
            Could not read settings from the app: {loadError}
          </p>
        ) : null}

        {!keySet ? (
          <p className="helios-settings__notice">
            Add an API key below to start chatting. Until then every message fails with a prompt to open Settings.
          </p>
        ) : null}

        <form className="helios-settings__form" onSubmit={handleSubmit}>
          <section className="helios-settings__card" aria-labelledby={connectionHeadingId}>
            <div className="helios-settings__section-heading">
              <h2 id={connectionHeadingId} className="helios-settings__section-title">
                Connection
              </h2>
              <p className="helios-settings__section-copy">Where Helios AI sends its requests.</p>
            </div>

            <div className="helios-settings__presets" role="group" aria-label="Provider presets">
              {PRESETS.map((preset) => (
                <Button
                  key={preset.label}
                  type="button"
                  variant="secondary"
                  size="small"
                  onClick={() => handlePreset(preset)}
                >
                  {preset.label}
                </Button>
              ))}
            </div>

            <div className="helios-settings__field">
              <label className="helios-settings__label" htmlFor={apiKeyId}>
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
              <span className="helios-settings__hint">
                {keySet ? "A key is already saved. Leave empty to keep it." : "Required before the first message."}
              </span>
              {keySet ? (
                <div>
                  <Button type="button" variant="tertiary" size="small" onClick={handleRemoveKey} disabled={saving}>
                    Remove saved key
                  </Button>
                </div>
              ) : null}
            </div>

            <div className="helios-settings__field">
              <label className="helios-settings__label" htmlFor={modelId}>
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

            <div className="helios-settings__field">
              <label className="helios-settings__label" htmlFor={baseUrlId}>
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
              <span className="helios-settings__hint">
                Point this at a compatible server or a local proxy.
              </span>
            </div>
          </section>

          <section className="helios-settings__card" aria-labelledby={generationHeadingId}>
            <div className="helios-settings__section-heading">
              <h2 id={generationHeadingId} className="helios-settings__section-title">
                Generation
              </h2>
              <p className="helios-settings__section-copy">How the model shapes its answers.</p>
            </div>

            <div className="helios-settings__field">
              <div className="helios-settings__label-row">
                <span className="helios-settings__label">Temperature</span>
                <span className="helios-settings__value" aria-live="polite">
                  {temperature.toFixed(1)}
                </span>
              </div>
              <Slider
                variant="continuous"
                min={0}
                max={2}
                step={0.1}
                value={[temperature]}
                onValueChange={(value) => setTemperature(value[0] ?? DEFAULT_TEMPERATURE)}
                aria-label="Temperature"
              />
              <span className="helios-settings__hint">
                Lower is more focused, higher is more creative. Default is 0.7.
              </span>
            </div>

            <div className="helios-settings__field">
              <label className="helios-settings__label" htmlFor={maxTokensId}>
                Max output tokens
              </label>
              <Input
                id={maxTokensId}
                type="number"
                min={1}
                step={1}
                inputMode="numeric"
                placeholder="Provider default"
                value={maxTokens}
                onChange={(event) => setMaxTokens(event.target.value)}
              />
              <span className="helios-settings__hint">
                Leave empty to let the provider decide the response length.
              </span>
            </div>

            <div className="helios-settings__field">
              <label className="helios-settings__label" htmlFor={instructionsId}>
                Custom instructions
              </label>
              <TextArea
                id={instructionsId}
                rows={4}
                placeholder="e.g. Start with a one-line summary, then explain the details."
                spellCheck={false}
                value={instructions}
                onChange={(event) => setInstructions(event.target.value)}
              />
              <span className="helios-settings__hint">
                Appended to the built-in system prompt for every conversation.
              </span>
              <div>
                <Button type="button" variant="tertiary" size="small" onClick={handleResetGeneration}>
                  Reset generation
                </Button>
              </div>
            </div>
          </section>

          <section className="helios-settings__card" aria-labelledby={appearanceHeadingId}>
            <div className="helios-settings__section-heading">
              <h2 id={appearanceHeadingId} className="helios-settings__section-title">
                Appearance
              </h2>
              <p className="helios-settings__section-copy">Light, dark, or follow your system.</p>
            </div>

            <div className="helios-settings__segmented" role="group" aria-label="Theme">
              {THEME_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={theme.preference === option.value}
                  className={`helios-settings__segment ${
                    theme.preference === option.value ? "is-active" : ""
                  }`}
                  onClick={() => theme.setPreference(option.value)}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </section>

          <section className="helios-settings__card" aria-labelledby={toolsHeadingId}>
            <div className="helios-settings__section-heading">
              <h2 id={toolsHeadingId} className="helios-settings__section-title">
                Tools
              </h2>
              <p className="helios-settings__section-copy">Native capabilities the model may call.</p>
            </div>

            <SwitchGroup>
              <SwitchItem
                label="Live weather"
                description="Let Helios AI fetch current conditions from Open-Meteo. No API key needed."
                checked={weatherTool}
                onChange={setWeatherTool}
              />
            </SwitchGroup>
          </section>

          <div className="helios-settings__actions">
            <Button type="submit" variant="primary" size="small" disabled={saving}>
              {saving ? "Saving…" : "Save changes"}
            </Button>
            {status ? (
              <p
                className={`helios-settings__status ${status.tone === "ok" ? "is-ok" : "is-error"}`}
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
