import { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { bytesToText, readRemoteFile, textToBytes, writeRemoteFile } from "@/services/filesService";
import type { RemoteFileEntry } from "@/types/files";
import "./AddServerModal.css";
import "./forms.css";

/** Above this, decoding the whole file into a textarea isn't a good idea - point at the terminal instead. */
const MAX_EDITABLE_SIZE = 1024 * 1024;

interface FileEditorPanelProps {
  serverId: string;
  entry: RemoteFileEntry;
  onClose: () => void;
}

export function FileEditorPanel({ serverId, entry, onClose }: FileEditorPanelProps) {
  const tooLarge = entry.size > MAX_EDITABLE_SIZE;
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(!tooLarge);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (tooLarge) return;
    setLoading(true);
    setError(null);
    readRemoteFile(serverId, entry.path)
      .then((bytes) => setContent(bytesToText(bytes)))
      .catch((err) => setError(err instanceof Error ? err.message : "Couldn't read this file."))
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId, entry.path]);

  async function handleSave() {
    setSaving(true);
    setError(null);
    try {
      await writeRemoteFile(serverId, entry.path, textToBytes(content));
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Couldn't save this file.");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-panel" style={{ width: 640 }} onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{entry.name}</h2>
          <button className="modal-close" onClick={onClose} aria-label="Close">
            <Icon name="x" size={16} />
          </button>
        </div>
        <div className="modal-body">
          {tooLarge ? (
            <p className="form-note">
              This file is larger than 1&nbsp;MB - too large to edit here. Use the terminal to work with it instead.
            </p>
          ) : loading ? (
            <p className="form-note">Loading...</p>
          ) : (
            <textarea
              className="form-input form-textarea"
              style={{ height: 360, fontFamily: "var(--font-mono)" }}
              value={content}
              onChange={(e) => setContent(e.target.value)}
              spellCheck={false}
            />
          )}

          {error && (
            <p className="form-note" style={{ color: "var(--danger)" }}>
              {error}
            </p>
          )}

          <div className="form-actions" style={{ marginTop: 12, gap: 8 }}>
            <Button variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            {!tooLarge && (
              <Button onClick={handleSave} disabled={loading || saving}>
                Save
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
