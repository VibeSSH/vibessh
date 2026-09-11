import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import "./LocalMachineCard.css";

/**
 * "This machine" - the local path, made visible.
 *
 * VibeSSH has always been able to run an Application on the desktop itself:
 * an Application with no `server_id` uses `LocalProcessRuntime` and never
 * opens an SSH session. The only way to reach it, though, was to leave the
 * server unselected in the creation wizard - a capability discoverable only
 * by not doing something, which is not discoverable at all.
 *
 * So it gets a place on the page that lists where Applications can run.
 *
 * Deliberately **not** a Node. Every node feature - Docker, the firewall,
 * WireGuard, the file browser, the terminal - is implemented over
 * `SshSession`, so a "localhost node" would either need an SSH server running
 * on the desktop (off by default on Windows) or a second implementation of
 * all of it. This card promises only what already works, which is why it
 * offers one action rather than the six a `ServerCard` does.
 */
export function LocalMachineCard() {
  const { t } = useTranslation();
  const navigate = useNavigate();

  return (
    <div className="local-machine-card">
      <div className="local-machine-card-icon">
        <Icon name="box" size={16} />
      </div>
      <div className="local-machine-card-body">
        <p className="local-machine-card-title">{t("localMachine.title")}</p>
        <p className="local-machine-card-description">{t("localMachine.description")}</p>
      </div>
      <Button variant="secondary" size="sm" onClick={() => navigate("/applications")}>
        <Icon name="plus" size={14} />
        {t("localMachine.action")}
      </Button>
    </div>
  );
}
