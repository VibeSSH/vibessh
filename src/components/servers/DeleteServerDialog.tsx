import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import "./AddServerModal.css";
import "./forms.css";

interface DeleteServerDialogProps {
  serverName: string;
  busy: boolean;
  error: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}

export function DeleteServerDialog({ serverName, busy, error, onConfirm, onCancel }: DeleteServerDialogProps) {
  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal-panel" style={{ width: 400 }} onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">Remove server</h2>
          <button className="modal-close" onClick={onCancel} aria-label="Close">
            <Icon name="x" size={16} />
          </button>
        </div>
        <div className="modal-body">
          <p style={{ margin: "0 0 16px", fontSize: 13, color: "var(--text-primary)", lineHeight: 1.5 }}>
            Remove <strong>{serverName}</strong>? Its stored password or key
            passphrase is deleted from your OS credential store too. This
            can&rsquo;t be undone.
          </p>
          {error && (
            <p className="form-note" style={{ color: "var(--danger)", marginBottom: 12 }}>
              {error}
            </p>
          )}
          <div className="form-actions" style={{ gap: 8 }}>
            <Button variant="secondary" onClick={onCancel} disabled={busy}>
              Cancel
            </Button>
            <Button variant="danger" onClick={onConfirm} disabled={busy}>
              Remove
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
