import { useTranslation } from "react-i18next";
import { Icon } from "@/components/ui/Icon";
import type { AgentCapabilities } from "@/types/pairing";
import "./CapabilityBadges.css";

const CAPABILITY_META: Record<keyof AgentCapabilities, { labelKey: string; icon: string }> = {
  terminal: { labelKey: "capabilities.terminal", icon: "terminal" },
  fileAccess: { labelKey: "capabilities.fileAccess", icon: "folder" },
  systemd: { labelKey: "capabilities.systemd", icon: "settings" },
  docker: { labelKey: "capabilities.docker", icon: "layout-grid" },
  minecraft: { labelKey: "capabilities.minecraft", icon: "sparkles" },
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
  const { t } = useTranslation();
  return (
    <div className="capability-badges">
      {ORDER.map((key) => {
        const supported = capabilities[key];
        const meta = CAPABILITY_META[key];
        return (
          <span key={key} className={`capability-badge ${supported ? "capability-badge-on" : "capability-badge-off"}`}>
            <Icon name={meta.icon} size={12} />
            {t(meta.labelKey)}
          </span>
        );
      })}
    </div>
  );
}
