import { Icon } from "@/components/ui/Icon";
import type { AgentCapabilities } from "@/types/pairing";
import "./CapabilityBadges.css";

const CAPABILITY_META: Record<keyof AgentCapabilities, { label: string; icon: string }> = {
  terminal: { label: "Terminal", icon: "terminal" },
  fileAccess: { label: "File access", icon: "folder" },
  systemd: { label: "Systemd", icon: "settings" },
  docker: { label: "Docker", icon: "layout-grid" },
  minecraft: { label: "Minecraft", icon: "sparkles" },
};

const ORDER: (keyof AgentCapabilities)[] = ["terminal", "fileAccess", "systemd", "docker", "minecraft"];

interface CapabilityBadgesProps {
  capabilities: AgentCapabilities;
}

/**
 * Etap I: shows what this host actually supports, not what every Linux box
 * is assumed to have - unsupported ones are shown struck through rather
 * than hidden, so it's visible *that* something was checked and found
 * absent instead of looking like the check never happened.
 */
export function CapabilityBadges({ capabilities }: CapabilityBadgesProps) {
  return (
    <div className="capability-badges">
      {ORDER.map((key) => {
        const supported = capabilities[key];
        const meta = CAPABILITY_META[key];
        return (
          <span key={key} className={`capability-badge ${supported ? "capability-badge-on" : "capability-badge-off"}`}>
            <Icon name={meta.icon} size={12} />
            {meta.label}
          </span>
        );
      })}
    </div>
  );
}
