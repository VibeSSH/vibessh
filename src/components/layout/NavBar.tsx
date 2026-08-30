import { NavLink } from "react-router-dom";
import { Icon } from "@/components/ui/Icon";
import { useRipple } from "@/hooks/useRipple";
import { primaryNav, moduleNav } from "@/config/navigation";
import type { NavModule } from "@/types/common";
import "./NavBar.css";

function NavTab({ item }: { item: NavModule }) {
  const { createRipple, rippleEls } = useRipple();
  return (
    <NavLink
      to={item.path}
      end={item.path === "/"}
      className={({ isActive }) => `navbar-tab ripple-host ${isActive ? "navbar-tab-active" : ""}`}
      onPointerDown={createRipple}
    >
      {rippleEls}
      <Icon name={item.icon} size={15} />
      <span>{item.label}</span>
    </NavLink>
  );
}

/**
 * Horizontal top tab bar, replacing the earlier vertical sidebar - ported
 * from Voltius's own NavBar (voltius/src/components/layout/NavBar.tsx):
 * icon + label per tab, a 2px accent underline on the active one, a ripple
 * on press (their own NavTabButton wires the same useRipple hook), no
 * filled background otherwise. Voltius uses this to switch between sections
 * *within* a vault (Hosts/Keychain/Port Forwarding/...); VibeSSH has no
 * vault concept, so this carries the app's actual top-level sections
 * instead (the old primaryNav + moduleNav sidebar groups, unchanged
 * otherwise).
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
            <NavTab key={item.id} item={item} />
          ),
        )}
      </div>
    </nav>
  );
}
