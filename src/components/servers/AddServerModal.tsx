import { useState } from "react";
import { Icon } from "@/components/ui/Icon";
import { SshServerForm } from "./SshServerForm";
import { AgentPairingFlow } from "./AgentPairingFlow";
import "./AddServerModal.css";

type Tab = "ssh" | "agent";

interface AddServerModalProps {
  onClose: () => void;
}

export function AddServerModal({ onClose }: AddServerModalProps) {
  const [tab, setTab] = useState<Tab>("ssh");

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-panel" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">Add server</h2>
          <button className="modal-close" onClick={onClose} aria-label="Close">
            <Icon name="x" size={16} />
          </button>
        </div>

        <div className="modal-tabs">
          <button
            className={`modal-tab ${tab === "ssh" ? "modal-tab-active" : ""}`}
            onClick={() => setTab("ssh")}
          >
            <Icon name="terminal" size={16} />
            Connect with SSH
          </button>
          <button
            className={`modal-tab ${tab === "agent" ? "modal-tab-active" : ""}`}
            onClick={() => setTab("agent")}
          >
            <Icon name="zap" size={16} />
            Install Vibe Agent
          </button>
        </div>

        <div className="modal-body">
          {tab === "ssh" ? <SshServerForm /> : <AgentPairingFlow onPaired={onClose} />}
        </div>
      </div>
    </div>
  );
}
