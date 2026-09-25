import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { useModalDialog } from "@/hooks/useModalDialog";
import { useServersStore } from "@/stores/serversStore";
import { ServerIconPicker } from "@/components/servers/ServerIconPicker";
import { SshServerForm } from "./SshServerForm";
import { AgentPairingFlow } from "./AgentPairingFlow";
import { NodeSetupWizard } from "./NodeSetupWizard";
import type { ManagedServer } from "@/stores/serversStore";
import type { ServerModalActivity } from "@/stores/serverModalStore";
import "./AddServerModal.css";

type Tab = "ssh" | "agent";

interface AddServerModalProps {
  onClose: () => void;
  /** Present => edit an existing SSH-mode server instead of adding a new one; hides the tabs. */
  editingServer?: ManagedServer;
  /** Passed down to each step so the dialog knows when closing it must hide
   *  work in flight rather than drop it - see `GlobalServerModal`. */
  onActivity?: (activity: Partial<ServerModalActivity>) => void;
}

export function AddServerModal({ onClose, editingServer, onActivity }: AddServerModalProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<Tab>("ssh");
  const isEditing = Boolean(editingServer);
  const upsertServer = useServersStore((s) => s.upsertServer);
  // The live row rather than `editingServer`, which is a snapshot taken when
  // the modal opened. The icon picker saves straight away, so reading the
  // snapshot left its preview on the old icon until the modal was reopened -
  // and spreading it back into the store overwrote anything that had changed
  // in the meantime.
  const liveServer = useServersStore((s) => s.servers.find((server) => server.id === editingServer?.id)) ?? editingServer;
  const backdrop = useModalDialog(onClose, { labelledBy: "addservermodal-dialog-title-1" });
  // A freshly created SSH-mode Node still needs Docker/WireGuard/ufw/Vibe
  // Network - the design doc's own "Setup Page" - so a brand new server
  // hands off into that guided flow instead of just closing (an edit, or a
  // freshly paired Agent-mode Node whose install already covers the same
  // groundwork, both still just close as before).
  const [justCreatedServer, setJustCreatedServer] = useState<ManagedServer | null>(null);

  if (justCreatedServer) {
    return <NodeSetupWizard serverId={justCreatedServer.id} serverName={justCreatedServer.name} onClose={onClose} onActivity={onActivity} />;
  }

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="addservermodal-dialog-title-1">{isEditing ? t("addServerModal.titleEdit") : t("addServerModal.titleAdd")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>

        {!isEditing && (
          <div className="modal-tabs">
            <button
              className={`modal-tab ${tab === "ssh" ? "modal-tab-active" : ""}`}
              onClick={() => setTab("ssh")}
            >
              <Icon name="terminal" size={16} />
              {t("addServerModal.tabSsh")}
            </button>
            <button
              className={`modal-tab ${tab === "agent" ? "modal-tab-active" : ""}`}
              onClick={() => setTab("agent")}
            >
              <Icon name="zap" size={16} />
              {t("addServerModal.tabAgent")}
            </button>
          </div>
        )}

        <div className="modal-body">
          {/* Only when editing: a node has to exist before it can be given an
              icon, and the picker saves immediately rather than waiting for
              the form's own Save - it writes a different column through a
              different command. */}
          {isEditing && liveServer && (
            <ServerIconPicker
              server={liveServer}
              onChanged={(icon) => upsertServer({ ...liveServer, icon: icon ?? undefined })}
            />
          )}
          {isEditing || tab === "ssh" ? (
            <SshServerForm
              editingServer={editingServer}
              onActivity={onActivity}
              onSaved={(server) => {
                if (isEditing) {
                  onClose();
                  return;
                }
                setJustCreatedServer(server);
              }}
            />
          ) : (
            <AgentPairingFlow onPaired={onClose} onActivity={onActivity} />
          )}
        </div>
      </div>
    </div>
  );
}
