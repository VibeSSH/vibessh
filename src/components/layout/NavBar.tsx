import { NavLink } from "react-router-dom";
import { Icon } from "@/components/ui/Icon";
import { primaryNav, moduleNav } from "@/config/navigation";
import "./NavBar.css";

/**
 * Horizontal top tab bar, replacing the earlier vertical sidebar - ported
 * from Voltius's own NavBar (voltius/src/components/layout/NavBar.tsx):
 * icon + label per tab, a 2px accent underline on the active one, no filled
 * background. Voltius uses this to switch between sections *within* a
 * vault (Hosts/Keychain/Port Forwarding/...); VibeSSH has no vault concept,
 * so this carries the app's actual top-level sections instead (the old
 * primaryNav + moduleNav sidebar groups, unchanged otherwise).
 */
export function NavBar() {
  const items = [...primaryNav, ...moduleNav];

  return (
    <nav className="navbar">
      <div className="navbar-brand">
        <img src="/vibessh-mark.svg" alt="" className="navbar-brand-mark" />
        <span className="navbar-brand-name">VibeSSH</span>
      </div>
      <div className="navbar-tabs">
        {items.map((item) =>
          item.comingSoon ? (
            <span key={item.id} className="navbar-tab navbar-tab-disabled" title="Coming soon">
              <Icon name={item.icon} size={15} />
              <span>{item.label}</span>
              <span className="navbar-tab-soon">soon</span>
            </span>
          ) : (
            <NavLink
              key={item.id}
              to={item.path}
              end={item.path === "/"}
              className={({ isActive }) => `navbar-tab ${isActive ? "navbar-tab-active" : ""}`}
            >
              <Icon name={item.icon} size={15} />
              <span>{item.label}</span>
            </NavLink>
          ),
        )}
      </div>
    </nav>
  );
}
