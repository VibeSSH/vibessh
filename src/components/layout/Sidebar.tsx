import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { NavLink } from "react-router-dom";
import { Icon } from "@/components/ui/Icon";
import { Tooltip } from "@/components/ui/Tooltip";
import { useRipple } from "@/hooks/useRipple";
import { sidebarGroups } from "@/config/navigation";
import { useAuthStore } from "@/stores/authStore";
import type { NavModule } from "@/types/common";
import "./Sidebar.css";

const COLLAPSED_KEY = "vibessh_sidebar_collapsed";
const EXPANDED_GROUPS_KEY = "vibessh_sidebar_expanded_groups";

function readStoredBoolean(key: string, fallback: boolean): boolean {
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? fallback : raw === "1";
  } catch {
    return fallback;
  }
}

function readStoredGroups(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(EXPANDED_GROUPS_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function NavItem({ item, collapsed }: { item: NavModule; collapsed: boolean }) {
  const { t } = useTranslation();
  const { createRipple, rippleEls } = useRipple();
  const label = t(item.labelKey);

  return (
    <NavLink
      to={item.path}
      end={item.path === "/"}
      className={({ isActive }) => `sidebar-link ripple-host ${isActive ? "sidebar-link-active" : ""}`}
      onPointerDown={createRipple}
      title={collapsed ? label : undefined}
    >
      {rippleEls}
      <Icon name={item.icon} size={16} className="sidebar-link-icon" />
      {!collapsed && <span className="sidebar-link-label">{label}</span>}
    </NavLink>
  );
}

function SidebarGroupSection({
  group,
  collapsed,
  expanded,
  onToggle,
}: {
  group: (typeof sidebarGroups)[number];
  collapsed: boolean;
  expanded: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation();
  const isSignedIn = useAuthStore((s) => s.status === "signedIn");
  const visibleItems = group.items.filter((item) => !item.requiresAuth || isSignedIn);
  if (visibleItems.length === 0) return null;

  return (
    <div className="sidebar-group">
      {!collapsed && (
        <button className="sidebar-group-header" onClick={onToggle} aria-expanded={expanded}>
          <span>{t(group.labelKey)}</span>
          <Icon name="chevron-down" size={13} className={`sidebar-group-chevron ${expanded ? "" : "sidebar-group-chevron-collapsed"}`} />
        </button>
      )}
      {(collapsed || expanded) && (
        <div className="sidebar-group-items">
          {visibleItems.map((item) => (
            <NavItem key={item.id} item={item} collapsed={collapsed} />
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * Real grouped, collapsible sidebar navigation - replaces the flat
 * horizontal NavBar tab row. That row fit VibeSSH's ~7 pages; the target
 * information architecture (Main/Workspace, and eventually Infrastructure/
 * Team/Other as their backing features ship) needs nested groups a single
 * row of tabs has no room for. Both the sidebar's own collapsed state and
 * each group's expanded/collapsed state persist to localStorage - genuine
 * per-device UI state, not app data, so localStorage is the right place for
 * it (see the production roadmap's Frontend State section).
 */
export function Sidebar() {
  const { t } = useTranslation();
  const [collapsed, setCollapsed] = useState(() => readStoredBoolean(COLLAPSED_KEY, false));
  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>(() => readStoredGroups());
  const navRef = useRef<HTMLElement>(null);
  const { createRipple, rippleEls } = useRipple();
  const isSignedIn = useAuthStore((s) => s.status === "signedIn");
  const visibleGroups = sidebarGroups.filter((group) => !group.requiresAuth || isSignedIn);

  useEffect(() => {
    try {
      localStorage.setItem(COLLAPSED_KEY, collapsed ? "1" : "0");
    } catch {
      // localStorage unavailable (private browsing, disabled site data) - the
      // toggle still works for this session, it just won't persist.
    }
  }, [collapsed]);

  useEffect(() => {
    try {
      localStorage.setItem(EXPANDED_GROUPS_KEY, JSON.stringify(expandedGroups));
    } catch {
      // Same as above - non-fatal if this can't be saved.
    }
  }, [expandedGroups]);

  function toggleGroup(id: string) {
    setExpandedGroups((prev) => ({ ...prev, [id]: prev[id] === false ? true : false }));
  }

  function isGroupExpanded(id: string): boolean {
    return expandedGroups[id] !== false;
  }

  function handleKeyDown(event: React.KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const container = navRef.current;
    if (!container) return;
    const links = Array.from(container.querySelectorAll<HTMLAnchorElement>("a.sidebar-link"));
    if (links.length === 0) return;
    const currentIndex = links.indexOf(document.activeElement as HTMLAnchorElement);
    event.preventDefault();
    const nextIndex =
      event.key === "ArrowDown"
        ? (currentIndex + 1 + links.length) % links.length
        : (currentIndex - 1 + links.length) % links.length;
    links[nextIndex].focus();
  }

  return (
    <nav
      ref={navRef}
      className={`sidebar ${collapsed ? "sidebar-collapsed" : ""}`}
      onKeyDown={handleKeyDown}
      aria-label={t("nav.sidebarAria")}
    >
      <div className="sidebar-scroll">
        {visibleGroups.map((group) => (
          <SidebarGroupSection
            key={group.id}
            group={group}
            collapsed={collapsed}
            expanded={isGroupExpanded(group.id)}
            onToggle={() => toggleGroup(group.id)}
          />
        ))}
      </div>

      <div className="sidebar-footer">
        <Tooltip label={collapsed ? t("nav.expand") : t("nav.collapse")} placement="right">
          <button
            className="sidebar-collapse-btn ripple-host"
            onClick={() => setCollapsed((c) => !c)}
            onPointerDown={createRipple}
            aria-label={collapsed ? t("nav.expand") : t("nav.collapse")}
          >
            {rippleEls}
            <Icon name={collapsed ? "chevrons-right" : "chevrons-left"} size={15} />
          </button>
        </Tooltip>
      </div>
    </nav>
  );
}
