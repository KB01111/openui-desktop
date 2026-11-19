import { AgentInterface, useThemeModePreference, WelcomeMascot } from "@openuidev/react-ui";
import { openuiLibrary } from "@openuidev/react-ui/genui-lib";
import { OpenUIWordmark } from "./components/openui-wordmark";
import { SettingsIcon } from "./components/settings-icon";
import { SettingsPage } from "./components/settings-page";
import { OPENUI_LOGO, STARTERS } from "./lib/starters";
import { tauriLLM } from "./lib/tauri-llm";
import { tauriStorage } from "./lib/tauri-storage";
import { useSettings } from "./lib/use-settings";

export function App() {
  const { setPreference, mode } = useThemeModePreference();
  const settings = useSettings();
  const apiKeySet = settings.snapshot?.apiKeySet === true;

  return (
    <div className="openui-desktop-shell">
      <AgentInterface
        llm={tauriLLM}
        storage={tauriStorage}
        componentLibrary={openuiLibrary}
        logoUrl={OPENUI_LOGO}
        agentName="OpenUI Desktop"
        theme={{ mode }}
        starters={STARTERS}
      >
        <AgentInterface.SidebarHeader agentName={<OpenUIWordmark mode={mode} />} />
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
          image={<WelcomeMascot />}
          title="What's on your mind today?"
          starterVariant="card"
          glowAnimation
        />
        <AgentInterface.Route path="settings">
          <SettingsPage settings={settings} />
        </AgentInterface.Route>
      </AgentInterface>
    </div>
  );
}
