import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { NavLink } from "react-router-dom";
import { Icon } from "@/components/ui/Icon";
import { Tooltip } from "@/components/ui/Tooltip";
import { useRipple } from "@/hooks/useRipple";
import { sidebarGroups } from "@/config/navigation";
import { useAuthStore } from "@/stores/authStore";
import { useServersStore } from "@/stores/serversStore";
import { useApplicationsStore } from "@/stores/applicationsStore";
import type { NavModule } from "@/types/common";
import { ConnectionSelector } from "./ConnectionSelector";
import { open } from "@tauri-apps/plugin-shell";
import "./Sidebar.css";

const COLLAPSED_KEY = "vibessh_sidebar_collapsed";
const EXPANDED_GROUPS_KEY = "vibessh_sidebar_expanded_groups";

/** Settings and the guide live pinned at the foot of the sidebar, separated
 * from the contextual navigation above - so they are excluded from the groups
 * and rendered on their own below. */
const FOOTER_ITEM_IDS = ["settings", "guide"];
const footerItems: NavModule[] = FOOTER_ITEM_IDS.map((id) => sidebarGroups.flatMap((g) => g.items).find((item) => item.id === id)).filter(
  (item): item is NavModule => Boolean(item),
);

/** Brand glyphs for the external links. lucide's bundled subset carries no
 * github/globe/discord marks, so these are inlined rather than pulled through
 * the Icon component - and drawn in currentColor so they inherit the row's
 * hover tint like every other sidebar glyph. */
function GlobeGlyph() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="10" />
      <path d="M2 12h20" />
      <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
    </svg>
  );
}
function GithubGlyph() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
    </svg>
  );
}
function DiscordGlyph() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M20.317 4.3698a19.7913 19.7913 0 0 0-4.8851-1.5152.0741.0741 0 0 0-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 0 0-.0785-.037 19.7363 19.7363 0 0 0-4.8852 1.515.0699.0699 0 0 0-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 0 0 .0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 0 0 .0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 0 0-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 0 1-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 0 1 .0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 0 1 .0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 0 1-.0066.1276 12.2986 12.2986 0 0 1-1.873.8914.0766.0766 0 0 0-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 0 0 .0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 0 0 .0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 0 0-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9554 2.4189-2.1568 2.4189Z" />
    </svg>
  );
}

const SOCIAL_LINKS: { id: string; url: string; label: string; glyph: React.ReactNode }[] = [
  { id: "web", url: "https://vibessh.dev", label: "vibessh.dev", glyph: <GlobeGlyph /> },
  { id: "github", url: "https://github.com/VibeSSH/vibessh", label: "GitHub", glyph: <GithubGlyph /> },
  { id: "discord", url: "https://discord.gg/vibessh", label: "Discord", glyph: <DiscordGlyph /> },
];

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

function NavItem({ item, collapsed, count }: { item: NavModule; collapsed: boolean; count?: number }) {
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
      {!collapsed && count != null && count > 0 && <span className="sidebar-link-count">{count}</span>}
    </NavLink>
  );
}

function SidebarGroupSection({
  group,
  collapsed,
  expanded,
  onToggle,
  counts,
}: {
  group: (typeof sidebarGroups)[number];
  collapsed: boolean;
  expanded: boolean;
  onToggle: () => void;
  counts: Record<string, number | undefined>;
}) {
  const { t } = useTranslation();
  const isSignedIn = useAuthStore((s) => s.status === "signedIn");
  const visibleItems = group.items.filter((item) => (!item.requiresAuth || isSignedIn) && !FOOTER_ITEM_IDS.includes(item.id));
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
            <NavItem key={item.id} item={item} collapsed={collapsed} count={counts[item.id]} />
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
  // Counts beside the Servers and Applications rows. Both stores are filled by
  // the Dashboard on launch (and kept current by their own pages), so the
  // numbers are there without the sidebar fetching anything of its own.
  const serverCount = useServersStore((s) => s.servers.length);
  const applicationCount = useApplicationsStore((s) => s.applications.length);
  const navCounts: Record<string, number | undefined> = { servers: serverCount, applications: applicationCount };

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
      {!collapsed && <ConnectionSelector />}

      <div className="sidebar-scroll">
        {visibleGroups.map((group) => (
          <SidebarGroupSection
            key={group.id}
            group={group}
            collapsed={collapsed}
            expanded={isGroupExpanded(group.id)}
            onToggle={() => toggleGroup(group.id)}
            counts={navCounts}
          />
        ))}
      </div>

      <div className="sidebar-footer-nav">
        {footerItems.map((item) => (
          <NavItem key={item.id} item={item} collapsed={collapsed} />
        ))}
      </div>

      <div className="sidebar-bottom">
        <div className="sidebar-social">
          {SOCIAL_LINKS.map((link) => (
            <Tooltip key={link.id} label={link.label} placement="right">
              <button
                type="button"
                className="sidebar-social-btn"
                onClick={() => {
                  open(link.url).catch(() => {});
                }}
                aria-label={link.label}
              >
                {link.glyph}
              </button>
            </Tooltip>
          ))}
        </div>
        <Tooltip label={collapsed ? t("nav.expand") : t("nav.collapse")} placement="right">
          <button
            className="sidebar-collapse-btn ripple-host"
            onClick={() => setCollapsed((c) => !c)}
            onPointerDown={createRipple}
            aria-label={collapsed ? t("nav.expand") : t("nav.collapse")}
          >
            {rippleEls}
            <Icon name={collapsed ? "chevrons-right" : "chevrons-left"} size={15} />
            {!collapsed && <span className="sidebar-collapse-label">{t("nav.collapse")}</span>}
          </button>
        </Tooltip>
      </div>
    </nav>
  );
}
