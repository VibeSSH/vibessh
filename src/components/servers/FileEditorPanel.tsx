import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { IconButton } from "@/components/ui/IconButton";
import { openSearchPanel } from "@codemirror/search";
import type { EditorView } from "@codemirror/view";
import { searchExtensions, searchPhrases } from "@/components/servers/editorSearch";
import CodeMirror from "@uiw/react-codemirror";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { vibesshEditorTheme } from "./cmTheme";
import { languageExtensionFor } from "./editorLanguage";
import { useBlockingProblems } from "./fileProblems";
import { bytesToText, listRemoteDirectory, readRemoteFile, readRemoteFileWindow, textToBytes, writeRemoteFile } from "@/services/filesService";
import { useWindowFocus } from "@/hooks/useWindowFocus";
import { formatBytes } from "@/utils/formatBytes";
import { toastSuccess } from "@/stores/toastStore";
import type { RemoteFileEntry } from "@/types/files";
import "./forms.css";
import "./FileEditorPanel.css";
import { errorMessage } from "@/services/tauri";

/** Above this, decoding the whole file into a textarea isn't a good idea - point at the terminal instead. */
const MAX_EDITABLE_SIZE = 1024 * 1024;

/**
 * How much of an oversized file is fetched at a time.
 *
 * Matches the Application-files editor - the same decision, made once about
 * how much context is worth waiting for before the first screen appears.
 */
const WINDOW_SIZE = 512 * 1024;

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
  const { t } = useTranslation();
  // The live editor, so the header button can reach the same panel
  // Ctrl+F opens. Null until CodeMirror has mounted, which is why the
  // button is disabled rather than absent before then - a control that
  // appears late is harder to find than one that is briefly inert.
  const editorViewRef = useRef<EditorView | null>(null);
  const tooLarge = entry.size > MAX_EDITABLE_SIZE;
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(!tooLarge);
  // Bumped to ask the reading effect to run again - the effect owns the
  // fetch, so re-reading is a change to what it depends on.
  const [reloadToken, setReloadToken] = useState(0);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // How much of an oversized file has been pulled in, and how big it turned
  // out to be when last measured.
  const [loadedBytes, setLoadedBytes] = useState(0);
  const [knownSize, setKnownSize] = useState(entry.size);
  const [windowLoading, setWindowLoading] = useState(false);
  // Enough dirty tracking to answer one question: may this editor replace
  // what is on screen without asking? It is deliberately not the full
  // unsaved-changes handling the application editor has - just the part that
  // stops a refresh from throwing away something typed.
  const [savedContent, setSavedContent] = useState("");
  const [changedOnDisk, setChangedOnDisk] = useState(false);
  // What the file looked like when this editor last agreed with it, seeded
  // from the listing that opened it.
  const baseline = useRef({ size: entry.size, modifiedAt: entry.modifiedAt });
  // See ApplicationFileEditorPanel: a file that does not parse is not a
  // file worth writing.
  const blocking = useBlockingProblems(entry.name, content, t);
  const dirty = content !== savedContent;
  // Read inside a callback that outlives the render it was made in.
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;

  /**
   * Looks for an edit made somewhere else, when the window comes back.
   *
   * There is no command that stats one remote path, so this reads the parent
   * directory and picks the entry out of it - the same listing the browser
   * behind this editor would ask for anyway.
   *
   * Skipped for the windowed viewer, which holds a prefix of a file being
   * read in pieces; re-reading from the top would fight whoever is still
   * writing to it.
   */
  useWindowFocus(() => {
    if (tooLarge || loading || saving) return;
    const parent = entry.path.slice(0, entry.path.lastIndexOf("/")) || "/";
    listRemoteDirectory(serverId, parent)
      .then((listing) => {
        const now = listing.find((candidate) => candidate.path === entry.path);
        // Gone, most likely deleted. That belongs to the listing behind this
        // editor, not to a bar over the text.
        if (!now) return;
        if (now.size === baseline.current.size && now.modifiedAt === baseline.current.modifiedAt) return;
        baseline.current = { size: now.size, modifiedAt: now.modifiedAt };
        // Nothing of the user's to lose, so this just catches up quietly.
        if (!dirtyRef.current) {
          setReloadToken((n) => n + 1);
          return;
        }
        setChangedOnDisk(true);
      })
      .catch(() => undefined);
  });
  const extensions = useMemo(() => [...vibesshEditorTheme(), ...languageExtensionFor(entry.name, t), ...searchExtensions(searchPhrases(t))], [entry.name, t]);

  useEffect(() => {
    if (tooLarge) return;
    setLoading(true);
    setError(null);
    readRemoteFile(serverId, entry.path)
      .then((bytes) => {
        const text = bytesToText(bytes);
        setContent(text);
        setSavedContent(text);
        setChangedOnDisk(false);
      })
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId, entry.path, reloadToken]);

  /**
   * Pulls in the next window of a file too large to edit.
   *
   * Appends rather than replaces, so reading stays where it was and the
   * content only ever grows towards the whole file.
   */
  function loadMore() {
    setWindowLoading(true);
    setError(null);
    readRemoteFileWindow(serverId, entry.path, loadedBytes, WINDOW_SIZE)
      .then((window) => {
        setContent((previous) => previous + bytesToText(window.bytes));
        setLoadedBytes(window.nextOffset);
        setKnownSize(window.totalSize);
      })
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setWindowLoading(false));
  }

  // The first window arrives on its own; every one after it is asked for.
  // Deliberately keyed on the file rather than on `loadMore`, which changes
  // identity with every offset and would fetch the whole file in a loop.
  useEffect(() => {
    if (!tooLarge) return;
    setContent("");
    setLoadedBytes(0);
    setKnownSize(entry.size);
    setWindowLoading(true);
    setError(null);
    readRemoteFileWindow(serverId, entry.path, 0, WINDOW_SIZE)
      .then((window) => {
        setContent(bytesToText(window.bytes));
        setLoadedBytes(window.nextOffset);
        setKnownSize(window.totalSize);
      })
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setWindowLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId, entry.path, tooLarge]);

  async function handleSave() {
    if (blocking.length > 0) return;
    setSaving(true);
    setError(null);
    try {
      await writeRemoteFile(serverId, entry.path, textToBytes(content));
      setSavedContent(content);
      setChangedOnDisk(false);
      toastSuccess(t("fileEditor.savedToast", { name: entry.name }));
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="file-editor-tab">
      <div className="file-editor-tab-header">
        <button className="file-editor-tab-back" onClick={onClose} aria-label={t("fileEditor.backAria")}>
          <Icon name="chevron-left" size={16} />
          <span>{entry.name}</span>
        </button>
        <div className="file-editor-tab-actions">
          {error && <span className="file-editor-tab-error">{error}</span>}
          <IconButton
            icon="search"
            size="sm"
            onClick={() => editorViewRef.current && openSearchPanel(editorViewRef.current)}
            title={t("fileEditor.searchAria")}
            disabled={loading || tooLarge}
          />
          {!tooLarge && (
            <Button onClick={handleSave} disabled={loading || saving || blocking.length > 0}>
              {saving ? t("fileEditor.saving") : t("fileEditor.save")}
            </Button>
          )}
        </div>
      </div>

      {changedOnDisk && (
        <p className="file-editor-tab-changed">
          <Icon name="alert-triangle" size={14} />
          <span>{t("fileEditor.changedOnDisk")}</span>
          <Button variant="secondary" size="sm" onClick={() => setReloadToken((n) => n + 1)}>
            {t("fileEditor.reloadFromDisk")}
          </Button>
        </p>
      )}

      {blocking.length > 0 && (
        <p className="file-editor-tab-blocked">
          <Icon name="alert-triangle" size={14} />
          {t("fileEditor.blockedBySyntax", { line: blocking[0].line, message: blocking[0].message, count: blocking.length })}
        </p>
      )}

      <div className="file-editor-tab-body">
        {tooLarge ? (
          <>
            {/* Read-only, and said rather than merely disabled: writing back a
                partly loaded file would truncate everything after the part
                that is in. */}
            <p className="form-note">
              {t("fileEditor.windowNote", { loaded: formatBytes(loadedBytes), total: formatBytes(knownSize) })}
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
                  {windowLoading ? t("fileEditor.windowLoading") : t("fileEditor.loadMore")}
                </Button>
              </div>
            )}
          </>
        ) : loading ? (
          <p className="form-note">{t("fileEditor.loading")}</p>
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
    </div>
  );
}
