import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import CodeMirror from "@uiw/react-codemirror";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { vibesshEditorTheme } from "@/components/servers/cmTheme";
import { languageExtensionFor } from "@/components/servers/editorLanguage";
import { bytesToText, textToBytes } from "@/services/filesService";
import { readApplicationFile, saveApplicationFile } from "@/services/applicationFilesService";
import { toastSuccess } from "@/stores/toastStore";
import { FileHistoryModal } from "./FileHistoryModal";
import type { RemoteFileEntry } from "@/types/files";
import "@/components/servers/FileEditorPanel.css";
import "@/components/servers/forms.css";
import "./ApplicationFiles.css";

/** Matches services::application_files_service::MAX_EDITABLE_FILE_SIZE - the server enforces this too (the actual trust boundary), this is just so the UI doesn't even try. */
const MAX_EDITABLE_SIZE = 1024 * 1024;
const BACKUP_PREFERENCE_KEY = "vibessh_files_backup_before_save";

function readBackupPreference(): boolean {
  try {
    const raw = localStorage.getItem(BACKUP_PREFERENCE_KEY);
    return raw === null ? true : raw === "1";
  } catch {
    return true;
  }
}

interface ApplicationFileEditorPanelProps {
  applicationId: string;
  entry: RemoteFileEntry;
  onClose: () => void;
  onSaved: () => void;
}

/** A full-tab editor view, same "swap the whole content area" shape as the older Node Files editor (FileEditorPanel.tsx) - this is a separate component (not a generalization of that one) because it needs real dirty-state tracking, Ctrl+S, a close-confirmation, a backup-before-save toggle, and Version History, none of which the simpler Node Files editor needs. Reuses the same CodeMirror theme/language wiring and CSS directly rather than re-deriving them. */
export function ApplicationFileEditorPanel({ applicationId, entry, onClose, onSaved }: ApplicationFileEditorPanelProps) {
  const { t } = useTranslation();
  const tooLarge = entry.size > MAX_EDITABLE_SIZE;
  const [content, setContent] = useState("");
  const [savedContent, setSavedContent] = useState("");
  const [loading, setLoading] = useState(!tooLarge);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [backupBeforeSave, setBackupBeforeSave] = useState(readBackupPreference);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const extensions = useMemo(() => [...vibesshEditorTheme(), ...languageExtensionFor(entry.name)], [entry.name]);
  const dirty = content !== savedContent;

  const load = useCallback(() => {
    if (tooLarge) return;
    setLoading(true);
    setError(null);
    readApplicationFile(applicationId, entry.path)
      .then((bytes) => {
        const text = bytesToText(bytes);
        setContent(text);
        setSavedContent(text);
      })
      .catch((err) => setError(err instanceof Error ? err.message : t("applicationFileEditor.couldntRead")))
      .finally(() => setLoading(false));
  }, [applicationId, entry.path, tooLarge, t]);

  useEffect(load, [load]);

  const handleSave = useCallback(async () => {
    if (loading || saving || tooLarge) return;
    setSaving(true);
    setError(null);
    try {
      await saveApplicationFile(applicationId, entry.path, textToBytes(content), backupBeforeSave);
      setSavedContent(content);
      toastSuccess(t("applicationFileEditor.savedToast", { name: entry.name }));
      onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("applicationFileEditor.couldntSave"));
    } finally {
      setSaving(false);
    }
  }, [applicationId, entry.path, entry.name, content, backupBeforeSave, loading, saving, tooLarge, onSaved, t]);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        handleSave();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [handleSave]);

  function toggleBackupPreference() {
    const next = !backupBeforeSave;
    setBackupBeforeSave(next);
    try {
      localStorage.setItem(BACKUP_PREFERENCE_KEY, next ? "1" : "0");
    } catch {
      // Non-fatal if this can't persist - the toggle still works for this session.
    }
  }

  function handleBack() {
    if (dirty) {
      setConfirmDiscard(true);
      return;
    }
    onClose();
  }

  return (
    <div className="file-editor-tab">
      <div className="file-editor-tab-header">
        <button className="file-editor-tab-back" onClick={handleBack} aria-label={t("applicationFileEditor.backAria")}>
          <Icon name="chevron-left" size={16} />
          <span>
            {entry.name}
            {dirty && " •"}
          </span>
        </button>
        <div className="file-editor-tab-actions">
          {error && <span className="file-editor-tab-error">{error}</span>}
          {!tooLarge && (
            <Checkbox checked={backupBeforeSave} onChange={toggleBackupPreference} label={t("applicationFileEditor.backupBeforeSave")} />
          )}
          <IconButton icon="history" size="sm" onClick={() => setHistoryOpen(true)} title={t("applicationFileEditor.historyAria")} />
          <IconButton icon="refresh-cw" size="sm" onClick={load} title={t("applicationFileEditor.reloadAria")} disabled={loading} />
          {!tooLarge && (
            <Button onClick={handleSave} disabled={loading || saving || !dirty}>
              {saving ? t("applicationFileEditor.saving") : t("applicationFileEditor.save")}
            </Button>
          )}
        </div>
      </div>

      <div className="file-editor-tab-body">
        {tooLarge ? (
          <p className="form-note">{t("applicationFileEditor.tooLarge")}</p>
        ) : loading ? (
          <p className="form-note">{t("applicationFileEditor.loading")}</p>
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

      {historyOpen && (
        <FileHistoryModal
          applicationId={applicationId}
          path={entry.path}
          fileName={entry.name}
          onClose={() => setHistoryOpen(false)}
          onRestored={() => {
            setHistoryOpen(false);
            load();
            onSaved();
          }}
        />
      )}

      {confirmDiscard && (
        <div className="modal-backdrop" onClick={() => setConfirmDiscard(false)}>
          <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">{t("applicationFileEditor.discardTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setConfirmDiscard(false)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">{t("applicationFileEditor.discardBody", { name: entry.name })}</p>
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setConfirmDiscard(false)}>
                  {t("common.cancel")}
                </Button>
                <Button variant="danger" onClick={onClose}>
                  {t("applicationFileEditor.discard")}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
