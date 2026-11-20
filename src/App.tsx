import { AgentInterface, useThemeModePreference } from "@openuidev/react-ui";
import { openuiLibrary } from "@openuidev/react-ui/genui-lib";
import { HeliosWordmark } from "./components/helios-wordmark";
import { SettingsIcon } from "./components/settings-icon";
import { SettingsPage } from "./components/settings-page";
import { HELIOS_LOGO, STARTERS } from "./lib/starters";
import { tauriLLM } from "./lib/tauri-llm";
import { tauriStorage } from "./lib/tauri-storage";
import { useSettings } from "./lib/use-settings";

export function App() {
  const { setPreference, preference, mode } = useThemeModePreference();
  const settings = useSettings();
  const apiKeySet = settings.snapshot?.apiKeySet === true;

  return (
    <div className="helios-shell">
      <AgentInterface
        llm={tauriLLM}
        storage={tauriStorage}
        componentLibrary={openuiLibrary}
        logoUrl={HELIOS_LOGO}
        agentName="Helios AI"
        theme={{ mode }}
        starters={STARTERS}
      >
        <AgentInterface.SidebarHeader agentName={<HeliosWordmark />} />
        <AgentInterface.SidebarFooter>
          <AgentInterface.SidebarItem
            path="settings"
            icon={<SettingsIcon />}
            trailing={apiKeySet ? undefined : "Set up"}
          >
            Settings
          </AgentInterface.SidebarItem>
          <AgentInterface.ThemeModeToggle mode={mode} onModeChange={setPreference} />
        </AgentInterface.SidebarFooter>
        <AgentInterface.ThreadHeader />
        <AgentInterface.Welcome
          image={<img className="helios-welcome-mark" src={HELIOS_LOGO} alt="" aria-hidden="true" />}
          title="What's on your mind today?"
          starterVariant="card"
          glowAnimation
        />
        <AgentInterface.Route path="settings">
          <SettingsPage settings={settings} theme={{ preference, setPreference }} />
        </AgentInterface.Route>
      </AgentInterface>
    </div>
  );
}
