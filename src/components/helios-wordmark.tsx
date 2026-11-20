// The Helios AI lockup: sun mark plus wordmark, sized by the sidebar header.
export function HeliosWordmark() {
  return (
    <div className="openui-agent-sidebar-header__agent-name helios-wordmark">
      <img className="helios-wordmark__mark" src="/helios-logo.svg" alt="" aria-hidden="true" />
      <span className="helios-wordmark__name">
        Helios<span className="helios-wordmark__ai">AI</span>
      </span>
    </div>
  );
}
