import { useEffect, useState } from "react";
import { getSettings, saveSettings } from "./tauri-storage";
import type { SettingsPatch, SettingsSnapshot } from "./contract";

/** Rust rejects `invoke` with its error string, not always an `Error`. */
export function describeError(cause: unknown) {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * One snapshot of the persisted settings, shared by the settings route and the
 * sidebar entry so both agree on whether a key exists. The key itself never
 * travels back — only `apiKeySet` does.
 */
export function useSettings() {
  const [snapshot, setSnapshot] = useState<SettingsSnapshot | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    getSettings()
      .then((next) => {
        if (active) setSnapshot(next);
      })
      .catch((cause: unknown) => {
        if (active) setLoadError(describeError(cause));
      });
    return () => {
      active = false;
    };
  }, []);

  const save = async (patch: SettingsPatch) => {
    const next = await saveSettings(patch);
    setSnapshot(next);
    setLoadError(null);
    return next;
  };

  return { snapshot, loadError, save };
}

export type SettingsController = ReturnType<typeof useSettings>;
