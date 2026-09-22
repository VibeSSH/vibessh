import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { openSearchPanel } from "@codemirror/search";
import type { EditorView } from "@codemirror/view";
import { searchExtensions, searchPhrases } from "@/components/servers/editorSearch";
import { useModalDialog } from "@/hooks/useModalDialog";
import CodeMirror from "@uiw/react-codemirror";
import { useEditorContextMenu } from "@/components/servers/useEditorContextMenu";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { vibesshEditorTheme } from "@/components/servers/cmTheme";
import { languageExtensionFor } from "@/components/servers/editorLanguage";
import { useBlockingProblems } from "@/components/servers/fileProblems";
import { bytesToText, textToBytes } from "@/services/filesService";
import { getApplicationFileMetadata, readApplicationFile, readApplicationFileWindow, saveApplicationFile } from "@/services/applicationFilesService";
import { useWindowFocus } from "@/hooks/useWindowFocus";
import { formatBytes } from "@/utils/formatBytes";
import { toastSuccess } from "@/stores/toastStore";
import { FileHistoryModal } from "./FileHistoryModal";
import type { RemoteFileEntry } from "@/types/files";
import "@/components/servers/FileEditorPanel.css";
import "@/components/servers/forms.css";
import "./ApplicationFiles.css";
import { errorMessage } from "@/services/tauri";

/** Matches services::application_files_service::MAX_EDITABLE_FILE_SIZE - the server enforces this too (the actual trust boundary), this is just so the UI doesn't even try. */
const MAX_EDITABLE_SIZE = 1024 * 1024;

/**
 * How much of an oversized file is fetched at a time.
 *
 * Large enough that a server log opens with plenty of context in one go,
 * small enough that the first window appears immediately even over a slow
 * link and that the choice to keep going stays the reader's.
 */
const WINDOW_SIZE = 512 * 1024;
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
  /** Reports unsaved-changes state up, so the file list beside the editor can
   *  guard a file switch with the same discard confirmation the back button
   *  uses - the parent owns which file is open, and only it can intercept a
   *  click on a different one before the editor is torn down. */
  onDirtyChange?: (dirty: boolean) => void;
}

/** A full-tab editor view, same "swap the whole content area" shape as the older Node Files editor (FileEditorPanel.tsx) - this is a separate component (not a generalization of that one) because it needs real dirty-state tracking, Ctrl+S, a close-confirmation, a backup-before-save toggle, and Version History, none of which the simpler Node Files editor needs. Reuses the same CodeMirror theme/language wiring and CSS directly rather than re-deriving them. */
export function ApplicationFileEditorPanel({ applicationId, entry, onClose, onSaved, onDirtyChange }: ApplicationFileEditorPanelProps) {
  const { t } = useTranslation();
  // The live editor, so the header button can reach the same panel
  // Ctrl+F opens. Null until CodeMirror has mounted, which is why the
  // button is disabled rather than absent before then - a control that
  // appears late is harder to find than one that is briefly inert.
  const editorViewRef = useRef<EditorView | null>(null);
  const tooLarge = entry.size > MAX_EDITABLE_SIZE;
  const editorMenu = useEditorContextMenu(editorViewRef, { editable: !tooLarge });
  const [content, setContent] = useState("");
  const [savedContent, setSavedContent] = useState("");
  const [loading, setLoading] = useState(!tooLarge);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [backupBeforeSave, setBackupBeforeSave] = useState(readBackupPreference);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  // How much of an oversized file has been pulled in so far, and how big it
  // turned out to be when last measured.
  const [loadedBytes, setLoadedBytes] = useState(0);
  const [knownSize, setKnownSize] = useState(entry.size);
  const [windowLoading, setWindowLoading] = useState(false);
  // Set when the file changed underneath an editor that has unsaved work in
  // it. Never acted on without asking: the whole point is that both versions
  // are somebody's, and picking one silently throws the other away.
  const [changedOnDisk, setChangedOnDisk] = useState(false);
  // What the file looked like when this editor last agreed with it. Seeded
  // from the listing, so a change is caught even if it happens before the
  // first check.
  const baseline = useRef({ size: entry.size, modifiedAt: entry.modifiedAt });
  const extensions = useMemo(() => [...vibesshEditorTheme(), ...languageExtensionFor(entry.name, t), ...searchExtensions(searchPhrases(t))], [entry.name, t]);
  const dirty = content !== savedContent;
  // Read inside a callback that outlives the render it was made in.
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  // Keep the parent's copy of the dirty flag current, so the file list beside
  // the editor knows whether switching files needs a discard confirmation.
  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);
  // A config file that does not parse is not a file worth writing: the
  // service reading it fails minutes later, somewhere else, with the
  // cause out of sight. Save is refused while that is true.
  const blocking = useBlockingProblems(entry.name, content, t);

  /**
   * Pulls in the next window of a file too large to edit.
   *
   * Appends rather than replaces, so reading stays where it was and the
   * content only ever grows towards the whole file.
   */
  const loadMore = useCallback(() => {
    setWindowLoading(true);
    setError(null);
    readApplicationFileWindow(applicationId, entry.path, loadedBytes, WINDOW_SIZE)
      .then((window) => {
        setContent((previous) => previous + bytesToText(window.bytes));
        setLoadedBytes(window.nextOffset);
        setKnownSize(window.totalSize);
      })
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setWindowLoading(false));
  }, [applicationId, entry.path, loadedBytes, t]);

  // The first window arrives on its own; every one after it is asked for.
  // Keyed on the file rather than on `loadMore`, which changes identity with
  // every offset and would fetch the whole file in a loop. Resets first, so
  // opening a second large file does not append to the previous one.
  useEffect(() => {
    if (!tooLarge) return;
    setContent("");
    setLoadedBytes(0);
    setKnownSize(entry.size);
    setWindowLoading(true);
    setError(null);
    readApplicationFileWindow(applicationId, entry.path, 0, WINDOW_SIZE)
      .then((window) => {
        setContent(bytesToText(window.bytes));
        setLoadedBytes(window.nextOffset);
        setKnownSize(window.totalSize);
      })
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setWindowLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [applicationId, entry.path, tooLarge]);

  /**
   * Records what the file looks like right now, so the next check compares
   * against this editor's own writes rather than reporting them as somebody
   * else's.
   */
  const refreshBaseline = useCallback(async () => {
    try {
      const meta = await getApplicationFileMetadata(applicationId, entry.path);
      baseline.current = { size: meta.size, modifiedAt: meta.modifiedAt };
    } catch {
      // Not worth surfacing: the worst case is one spurious "changed on
      // disk" notice, which asks rather than acts.
    }
  }, [applicationId, entry.path]);

  const load = useCallback(() => {
    if (tooLarge) return;
    setLoading(true);
    setError(null);
    readApplicationFile(applicationId, entry.path)
      .then((bytes) => {
        const text = bytesToText(bytes);
        setContent(text);
        setSavedContent(text);
        setChangedOnDisk(false);
        void refreshBaseline();
      })
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }, [applicationId, entry.path, tooLarge, refreshBaseline, t]);

  /**
   * Looks for an edit made somewhere else.
   *
   * Only when the window comes back, because that is when there is somebody
   * to tell. A file the app cannot stat any more is left alone - it was
   * probably deleted, and the listing behind this editor is where that
   * belongs, not a bar over the text.
   *
   * Skipped for the windowed viewer: it holds a prefix of a file being
   * re-read in pieces, and re-reading from the top on every change would
   * fight whoever is still writing to it.
   */
  const checkForExternalEdit = useCallback(() => {
    if (tooLarge || loading || saving) return;
    getApplicationFileMetadata(applicationId, entry.path)
      .then((meta) => {
        if (meta.size === baseline.current.size && meta.modifiedAt === baseline.current.modifiedAt) return;
        // Nothing of the user's to lose, so this just catches up quietly.
        if (!dirtyRef.current) {
          load();
          return;
        }
        setChangedOnDisk(true);
      })
      .catch(() => undefined);
  }, [applicationId, entry.path, tooLarge, loading, saving, load]);

  useWindowFocus(checkForExternalEdit);

  useEffect(load, [load]);

  const handleSave = useCallback(async () => {
    if (loading || saving || tooLarge || blocking.length > 0) return;
    setSaving(true);
    setError(null);
    try {
      await saveApplicationFile(applicationId, entry.path, textToBytes(content), backupBeforeSave);
      setSavedContent(content);
      setChangedOnDisk(false);
      void refreshBaseline();
      toastSuccess(t("applicationFileEditor.savedToast", { name: entry.name }));
      onSaved();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setSaving(false);
    }
  }, [applicationId, entry.path, entry.name, content, backupBeforeSave, loading, saving, tooLarge, blocking.length, onSaved, refreshBaseline, t]);

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
          <IconButton
            icon="search"
            size="sm"
            onClick={() => editorViewRef.current && openSearchPanel(editorViewRef.current)}
            title={t("applicationFileEditor.searchAria")}
            disabled={loading || tooLarge}
          />
          <IconButton icon="history" size="sm" onClick={() => setHistoryOpen(true)} title={t("applicationFileEditor.historyAria")} />
          <IconButton icon="refresh-cw" size="sm" onClick={load} title={t("applicationFileEditor.reloadAria")} disabled={loading} />
          {!tooLarge && (
            <Button onClick={handleSave} disabled={loading || saving || !dirty || blocking.length > 0}>
              {saving ? t("applicationFileEditor.saving") : t("applicationFileEditor.save")}
            </Button>
          )}
        </div>
      </div>

      {blocking.length > 0 && (
        <p className="file-editor-tab-blocked">
          <Icon name="alert-triangle" size={14} />
          {t("fileEditor.blockedBySyntax", { line: blocking[0].line, message: blocking[0].message, count: blocking.length })}
        </p>
      )}

      {changedOnDisk && (
        <p className="file-editor-tab-changed">
          <Icon name="alert-triangle" size={14} />
          <span>{t("applicationFileEditor.changedOnDisk")}</span>
          <Button variant="secondary" size="sm" onClick={load}>
            {t("applicationFileEditor.reloadFromDisk")}
          </Button>
        </p>
      )}

      {/* On the body rather than on CodeMirror itself: the editor is
          swapped between an editable and a read-only instance, and a
          right-click should behave the same over either. */}
      <div className="file-editor-tab-body" onContextMenu={editorMenu.onContextMenu}>
        {tooLarge ? (
          <>
            {/* Read-only, and said out loud rather than just disabled: a
                partly loaded file written back would truncate everything
                after the part that is in. */}
            <p className="form-note">
              {t("applicationFileEditor.windowNote", {
                loaded: formatBytes(loadedBytes),
                total: formatBytes(knownSize),
              })}
            </p>
            <CodeMirror
              className="file-editor-tab-codemirror"
              value={content}
              minHeight="320px"
              maxHeight="calc(100vh - 340px)"
              theme="none"
              extensions={extensions}
              editable={false}
              onCreateEditor={(view) => (editorViewRef.current = view)}
            />
            {loadedBytes < knownSize && (
              <div className="file-editor-window-actions">
                <Button variant="secondary" size="sm" onClick={loadMore} disabled={windowLoading}>
                  {windowLoading ? t("applicationFileEditor.windowLoading") : t("applicationFileEditor.loadMore")}
                </Button>
              </div>
            )}
          </>
        ) : loading ? (
          <p className="form-note">{t("applicationFileEditor.loading")}</p>
        ) : (
          <CodeMirror
            className="file-editor-tab-codemirror"
            value={content}
            // Not `height="100%"`: that resolves against the parent, and the
            // parent only has a height on the full-page Files view. Inside the
            // Application tab it resolved to auto, so the editor took the height
            // of the file and the window became the only scrollbar.
            //
            // A max height set on the editor itself is what makes CodeMirror
            // turn its own scroller on, whatever is above it.
            minHeight="320px"
            maxHeight="calc(100vh - 300px)"
            theme="none"
            extensions={extensions}
            onChange={setContent}
            onCreateEditor={(view) => (editorViewRef.current = view)}
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
        <DiscardChangesDialog fileName={entry.name} onCancel={() => setConfirmDiscard(false)} onDiscard={onClose} />
      )}
      {editorMenu.element}
    </div>
  );
}

/**
 * Its own component so `useModalDialog`'s focus effect runs on the dialog's
 * own mount - see `EnvironmentTab`'s copy of this note. This one also gains
 * a working Escape and a focus trap it never had: it is the confirmation
 * standing between the user and discarding their unsaved edits, so being
 * unable to reach its Cancel button by keyboard was the worst place in the
 * app for that gap.
 */
function DiscardChangesDialog({ fileName, onCancel, onDiscard }: { fileName: string; onCancel: () => void; onDiscard: () => void }) {
  const { t } = useTranslation();
  const dialog = useModalDialog(onCancel, { labelledBy: "discard-changes-title" });
  return (
    <div className="modal-backdrop" {...dialog.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...dialog.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="discard-changes-title">
            {t("applicationFileEditor.discardTitle")}
          </h2>
          <IconButton icon="x" size="sm" onClick={onCancel} title={t("common.close")} />
        </div>
        <div className="modal-body">
          <p className="dialog-body-text">{t("applicationFileEditor.discardBody", { name: fileName })}</p>
          <div className="form-actions">
            <Button variant="secondary" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={onDiscard}>
              {t("applicationFileEditor.discard")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
