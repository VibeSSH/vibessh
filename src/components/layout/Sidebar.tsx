import { NavLink } from "react-router-dom";
import { Icon } from "@/components/ui/Icon";
import { primaryNav, moduleNav } from "@/config/navigation";
import { useUiStore } from "@/stores/uiStore";
import "./Sidebar.css";

export function Sidebar() {
  const collapsed = useUiStore((state) => state.sidebarCollapsed);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);

  return (
    <aside className={`sidebar ${collapsed ? "sidebar-collapsed" : ""}`}>
      <div className="sidebar-brand">
        <img src="/vibessh-mark.svg" alt="" className="sidebar-brand-mark" />
        {!collapsed && <span className="sidebar-brand-name">VibeSSH</span>}
      </div>

      <nav className="sidebar-nav">
        <div className="sidebar-section">
          {primaryNav.map((item) => (
            <NavLink
              key={item.id}
              to={item.path}
              end={item.path === "/"}
              className={({ isActive }) => `sidebar-link ${isActive ? "sidebar-link-active" : ""}`}
            >
              <Icon name={item.icon} size={18} />
              {!collapsed && <span>{item.label}</span>}
            </NavLink>
          ))}
        </div>

        {!collapsed && <div className="sidebar-section-label">Modules</div>}
        <div className="sidebar-section">
          {moduleNav.map((item) => (
            <span key={item.id} className="sidebar-link sidebar-link-disabled" title="Coming soon">
              <Icon name={item.icon} size={18} />
              {!collapsed && (
                <>
                  <span>{item.label}</span>
                  <span className="sidebar-soon">soon</span>
                </>
              )}
            </span>
          ))}
        </div>
      </nav>

      <button className="sidebar-collapse-btn" onClick={toggleSidebar} aria-label="Toggle sidebar">
        <Icon name={collapsed ? "chevron-right" : "chevron-left"} size={16} />
      </button>
    </aside>
  );
}
