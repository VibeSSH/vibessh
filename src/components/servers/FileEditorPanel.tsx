import { useEffect, useMemo, useState } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { vibesshEditorTheme } from "./cmTheme";
import { languageExtensionFor } from "./editorLanguage";
import { bytesToText, readRemoteFile, textToBytes, writeRemoteFile } from "@/services/filesService";
import { toastSuccess } from "@/stores/toastStore";
import type { RemoteFileEntry } from "@/types/files";
import "./forms.css";
import "./FileEditorPanel.css";

/** Above this, decoding the whole file into a textarea isn't a good idea - point at the terminal instead. */
const MAX_EDITABLE_SIZE = 1024 * 1024;

interface FileEditorPanelProps {
  serverId: string;
  entry: RemoteFileEntry;
  onClose: () => void;
}

/**
 * A full-tab editor view, not a modal - Files.tsx swaps the whole content
 * area for this instead of overlaying it, matching Voltius's own editor
 * (voltius/src/components/filetransfer/editor/EditorTab.tsx) using the
 * full pane rather than a cramped dialog.
 */
export function FileEditorPanel({ serverId, entry, onClose }: FileEditorPanelProps) {
  const tooLarge = entry.size > MAX_EDITABLE_SIZE;
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(!tooLarge);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const extensions = useMemo(() => [...vibesshEditorTheme(), ...languageExtensionFor(entry.name)], [entry.name]);

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
      toastSuccess(`Saved ${entry.name}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Couldn't save this file.");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="file-editor-tab">
      <div className="file-editor-tab-header">
        <button className="file-editor-tab-back" onClick={onClose} aria-label="Back to file list">
          <Icon name="chevron-left" size={16} />
          <span>{entry.name}</span>
        </button>
        <div className="file-editor-tab-actions">
          {error && <span className="file-editor-tab-error">{error}</span>}
          {!tooLarge && (
            <Button onClick={handleSave} disabled={loading || saving}>
              {saving ? "Saving..." : "Save"}
            </Button>
          )}
        </div>
      </div>

      <div className="file-editor-tab-body">
        {tooLarge ? (
          <p className="form-note">This file is larger than 1&nbsp;MB - too large to edit here. Use the terminal to work with it instead.</p>
        ) : loading ? (
          <p className="form-note">Loading...</p>
        ) : (
          <CodeMirror
            className="file-editor-tab-codemirror"
            value={content}
            height="100%"
            theme="none"
            extensions={extensions}
            onChange={setContent}
            basicSetup={{ foldGutter: true, highlightActiveLine: true }}
          />
        )}
      </div>
    </div>
  );
}
