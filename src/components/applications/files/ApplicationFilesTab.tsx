import { memo, useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { queryKeys } from "@/services/queryKeys";
import { useTranslation } from "react-i18next";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Badge } from "@/components/ui/Badge";
import { GuideLink } from "@/guide/GuideLink";
import { Breadcrumbs } from "@/components/ui/Breadcrumbs";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { useContextMenu, type ContextMenuItem } from "@/components/ui/ContextMenu";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { OverflowMenu } from "@/components/ui/OverflowMenu";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { CreateEntryModal } from "@/components/servers/CreateEntryModal";
import { useFileDrop } from "@/hooks/useFileDrop";
import { useModalDialog } from "@/hooks/useModalDialog";
import { restartApplication } from "@/services/applicationService";
import {
  copyApplicationFile,
  createApplicationDirectory,
  deleteApplicationFile,
  downloadApplicationFile,
  extractApplicationArchive,
  getApplicationFileMetadata,
  onTransferProgress,
  renameApplicationFile,
  setApplicationFilePermissions,
  localPathIsDirectory,
  uploadApplicationDirectory,
  uploadApplicationFile,
  writeApplicationFile,
  listApplicationFiles,
} from "@/services/applicationFilesService";
import { useFileTransferStore } from "@/stores/fileTransferStore";
import { toastSuccess } from "@/stores/toastStore";
import type { ApplicationDetail, KnownFile } from "@/types/application";
import type { RemoteFileEntry } from "@/types/files";
import { ApplicationFileEditorPanel } from "./ApplicationFileEditorPanel";
import { ChmodModal } from "./ChmodModal";
import { JarReplaceWarningModal } from "./JarReplaceWarningModal";
import { RenameOrMoveModal } from "./RenameOrMoveModal";
import { TransferQueuePanel } from "./TransferQueuePanel";
import "@/components/servers/forms.css";
import "@/pages/Files.css";
import "./ApplicationFiles.css";
import { formatShortDate } from "@/utils/formatShortDate";
import { errorMessage } from "@/services/tauri";

/// Matches the Node Files page and the Actions page.
const MAX_ROWS_SHOWN = 200;

interface FileRowProps {
  entry: RemoteFileEntry;
  /** Passed in rather than read from i18n here, so a language change still reaches a memoised row. */
  language: string;
  onOpen: (entry: RemoteFileEntry) => void;
  onDownload: (entry: RemoteFileEntry) => void;
  onContextMenu: (event: ReactMouseEvent, entry: RemoteFileEntry) => void;
  buildMenuItems: (entry: RemoteFileEntry) => ContextMenuItem[];
}

/**
 * One file or directory.
 *
 * **Why this is a component of its own, and memoised.** This tab is rendered
 * inline by ApplicationDetail, which polls the application every five
 * seconds. Nothing in between was memoised, so every poll rebuilt all two
 * hundred rows - and a row is not cheap: an iconify SVG, a name button, an
 * IconButton that wraps itself in a Tooltip, an OverflowMenu, sometimes a
 * Badge. Well over a thousand components reconciled every five seconds, on
 * the same thread that is meant to be producing scroll frames.
 *
 * The memo only holds because the props above are stable: TanStack Query's
 * structural sharing keeps each `entry` identical across a refetch that did
 * not change it, and the four callbacks are pinned in the parent.
 */
const FileRow = memo(function FileRow({ entry, language, onOpen, onDownload, onContextMenu, buildMenuItems }: FileRowProps) {
  const { t } = useTranslation();
  return (
    <li className="server-list-item" onContextMenu={(event) => onContextMenu(event, entry)}>
      <div className="server-list-icon">
        <Icon name={entry.isDir ? "folder" : "file"} size={16} />
      </div>
      <button className="files-entry-name" title={entry.name} onClick={() => onOpen(entry)}>
        {entry.name}
        {entry.isSymlink && <Badge tone="neutral">{t("applicationFilesTab.symlink")}</Badge>}
      </button>
      <div className="application-files-entry-meta">
        {!entry.isDir && <span>{formatSize(entry.size)}</span>}
        {entry.modifiedAt && <span>{formatShortDate(entry.modifiedAt, language)}</span>}
        {entry.permissions !== undefined && <span>{formatOctal(entry.permissions)}</span>}
      </div>
      {!entry.isDir && (
        <IconButton
          icon="download"
          size="sm"
          title={t("applicationFilesTab.downloadAria", { name: entry.name })}
          onClick={() => onDownload(entry)}
        />
      )}
      {/* Built on open, not on render - see OverflowMenu's own note. This
          list can be two hundred rows, each with six translated menu labels
          nobody has asked to see. */}
      <OverflowMenu ariaLabel={t("applicationFilesTab.moreAria", { name: entry.name })} items={() => buildMenuItems(entry)} />
    </li>
  );
});

const ROOT_PATH = ".";

function joinPath(dir: string, name: string): string {
  return dir === ROOT_PATH ? name : `${dir}/${name}`;
}

/** Best-effort: looks for a `-jar <name>` pair in the rendered runtime_config's own `args` (the shape every non-Docker blueprint's render_runtime_config produces) - used only to decide whether an upload needs the JAR-replace warning (section 123), never anything security-sensitive. */
function extractCurrentJarName(runtimeConfig: unknown): string | null {
  if (!runtimeConfig || typeof runtimeConfig !== "object") return null;
  const args = (runtimeConfig as Record<string, unknown>).args;
  if (!Array.isArray(args)) return null;
  const jarIndex = args.findIndex((a) => a === "-jar");
  if (jarIndex < 0 || typeof args[jarIndex + 1] !== "string") return null;
  const jarArg = args[jarIndex + 1] as string;
  return jarArg.split("/").pop() ?? jarArg;
}

interface ApplicationFilesTabProps {
  applicationId: string;
  application: ApplicationDetail;
  knownFiles: KnownFile[];
}

/** design brief's "Application Files / SFTP" section, in full: browser (breadcrumbs/list/toolbar), a real transfer queue, the CodeMirror editor (dirty state, Ctrl+S, backup-before-save, Version History), rename/move/copy/chmod, zip extraction, Quick Files, and the JAR-replace warning. Deliberately not built here (documented, not silently dropped): drag & drop upload (native picker only - still real streaming, just not drag-initiated), and "compress selection into a new archive" (only *extracting* an existing one is implemented; the backend has no ZipWriter-based create-archive path yet). */
export function ApplicationFilesTab({ applicationId, application, knownFiles }: ApplicationFilesTabProps) {
  const { t, i18n } = useTranslation();
  const currentJarName = extractCurrentJarName(application.runtimeConfig);
  const isRunning = application.status === "running";

  const [path, setPath] = useState(ROOT_PATH);
  const [filter, setFilter] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);
  const [openFile, setOpenFile] = useState<RemoteFileEntry | null>(null);
  const [createModal, setCreateModal] = useState<"file" | "folder" | null>(null);
  const [renameTarget, setRenameTarget] = useState<{ entry: RemoteFileEntry; mode: "rename" | "move" | "copy" } | null>(null);
  const [chmodTarget, setChmodTarget] = useState<RemoteFileEntry | null>(null);
  const [deletingEntry, setDeletingEntry] = useState<RemoteFileEntry | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [extractingPath, setExtractingPath] = useState<string | null>(null);
  const [jarWarning, setJarWarning] = useState<{ fileName: string; localSrc: string; targetPath: string } | null>(null);
  const deleteBackdrop = useModalDialog(() => !deleteBusy && setDeletingEntry(null), { labelledBy: "applicationfilestab-dialog-title-1" });
  const contextMenu = useContextMenu();

  const addTransfer = useFileTransferStore((s) => s.addTransfer);
  const updateProgress = useFileTransferStore((s) => s.updateProgress);
  const markDone = useFileTransferStore((s) => s.markDone);
  const markError = useFileTransferStore((s) => s.markError);

  const queryClient = useQueryClient();

  // One cached answer per directory. Walking back up a tree you have already
  // walked down is now instant - the listing is on screen before the Node is
  // asked whether anything changed.
  const {
    data: entries = [],
    isPending: loading,
    error: loadError,
  } = useQuery({
    queryKey: queryKeys.applicationFiles(applicationId, path),
    queryFn: () => listApplicationFiles(applicationId, path),
  });

  const error = actionError ?? (loadError ? errorMessage(loadError, t) : null);

  /**
   * Navigate, or refresh where we already are.
   *
   * Changing the path is enough to change what is displayed, because the
   * path is part of the query key. Asking for the directory already shown
   * means somebody wants it read again, which is an invalidation.
   *
   * A directory that fails to list now leaves you in it, with the error and
   * an empty list, rather than silently keeping you in the previous one -
   * the breadcrumb and the message then agree about which directory could
   * not be read.
   */
  const load = useCallback(
    (targetPath: string) => {
      setActionError(null);
      if (targetPath === path) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.applicationFiles(applicationId, targetPath) });
        return;
      }
      setPath(targetPath);
    },
    [applicationId, path, queryClient],
  );

  // Same reasoning as the Node Files page: a directory can hold tens of
  // thousands of entries and rendering a row each locks the window up, but
  // a bare cap would make distant entries unreachable. Cap plus filter.
  /**
   * The filter the list is built from, one step behind the input.
   *
   * `useDeferredValue` keeps the text field responding to every keystroke
   * while the two hundred rows underneath it are re-rendered at a lower
   * priority. Without it each character re-rendered the whole list before
   * the character appeared, which is exactly what "typing here lags" is.
   */
  const deferredFilter = useDeferredValue(filter);

  const matchingEntries = useMemo(() => {
    const needle = deferredFilter.trim().toLowerCase();
    if (!needle) return entries;
    return entries.filter((entry) => entry.name.toLowerCase().includes(needle));
  }, [entries, deferredFilter]);
  const visibleEntries = matchingEntries.slice(0, MAX_ROWS_SHOWN);
  const truncated = matchingEntries.length > visibleEntries.length;

  // Back to the top when the application changes - the previous one's tree
  // says nothing about this one.
  useEffect(() => {
    setPath(ROOT_PATH);
  }, [applicationId]);

  const segments = path === ROOT_PATH ? [] : path.split("/").filter(Boolean);

  async function runUpload(localSrc: string, targetPath: string, isDirectory = false) {
    const transferId = crypto.randomUUID();
    const fileName = targetPath.split("/").pop() ?? targetPath;
    addTransfer({
      id: transferId,
      name: fileName,
      direction: "upload",
      total: 0,
      retry: () => runUpload(localSrc, targetPath, isDirectory),
    });
    const unlisten = await onTransferProgress(transferId, ({ transferred, total }) => updateProgress(transferId, transferred, total));
    try {
      // A folder keeps its shape on the far side, so the backend gets the
      // parent directory and works the rest out; a file gets its own
      // destination path.
      if (isDirectory) await uploadApplicationDirectory(applicationId, localSrc, path, transferId);
      else await uploadApplicationFile(applicationId, localSrc, targetPath, transferId);
      markDone(transferId);
      load(path);
    } catch (err) {
      markError(transferId, errorMessage(err, t));
    } finally {
      unlisten();
    }
  }

  async function runDownload(entry: RemoteFileEntry) {
    const localDest = await save({ defaultPath: entry.name, title: t("applicationFilesTab.downloadTitle") });
    if (!localDest) return;
    const transferId = crypto.randomUUID();
    addTransfer({ id: transferId, name: entry.name, direction: "download", total: entry.size, retry: () => runDownload(entry) });
    const unlisten = await onTransferProgress(transferId, ({ transferred, total }) => updateProgress(transferId, transferred, total));
    try {
      await downloadApplicationFile(applicationId, entry.path, localDest, transferId);
      markDone(transferId);
    } catch (err) {
      markError(transferId, errorMessage(err, t));
    } finally {
      unlisten();
    }
  }

  async function handleUpload() {
    const selected = await open({ multiple: true, title: t("applicationFilesTab.uploadTitle") });
    if (!selected) return;
    uploadPaths(Array.isArray(selected) ? selected : [selected]);
  }

  /**
   * Local paths, from the picker or from a drop - the two are the same thing
   * by the time they get here, so the JAR warning guards both. That warning
   * only makes sense for a single file: a drop of twenty is not somebody
   * carefully replacing the server jar.
   */
  async function uploadPaths(paths: string[]) {
    // Which of them are folders - asked once for the whole batch rather than
    // inside the loop below.
    const kinds = await Promise.all(paths.map((candidate) => localPathIsDirectory(candidate)));

    // The jar warning is about replacing one file; a dropped folder is never
    // that, so it goes straight through.
    if (paths.length === 1 && !kinds[0]) {
      const localSrc = paths[0];
      const fileName = localSrc.split(/[/\\]/).pop() ?? localSrc;
      const targetPath = joinPath(path, fileName);
      if (currentJarName && fileName === currentJarName) {
        setJarWarning({ fileName, localSrc, targetPath });
        return;
      }
      runUpload(localSrc, targetPath);
      return;
    }
    for (const [index, localSrc] of paths.entries()) {
      const fileName = localSrc.split(/[/\\]/).pop() ?? localSrc;
      runUpload(localSrc, joinPath(path, fileName), kinds[index]);
    }
  }

  // Only while the file list is on screen: the editor panel takes over the
  // whole tab, and dropping a file onto an open editor should not quietly
  // upload it somewhere behind that editor.
  const dragging = useFileDrop(uploadPaths, !openFile);

  async function handleJarUploadOnly() {
    if (!jarWarning) return;
    runUpload(jarWarning.localSrc, jarWarning.targetPath);
    setJarWarning(null);
  }

  async function handleJarUploadAndRestart() {
    if (!jarWarning) return;
    const { localSrc, targetPath } = jarWarning;
    setJarWarning(null);
    const transferId = crypto.randomUUID();
    const fileName = targetPath.split("/").pop() ?? targetPath;
    addTransfer({ id: transferId, name: fileName, direction: "upload", total: 0 });
    const unlisten = await onTransferProgress(transferId, ({ transferred, total }) => updateProgress(transferId, transferred, total));
    try {
      await uploadApplicationFile(applicationId, localSrc, targetPath, transferId);
      markDone(transferId);
      load(path);
      await restartApplication(applicationId);
      toastSuccess(t("applicationFilesTab.restartedToast"));
    } catch (err) {
      markError(transferId, errorMessage(err, t));
    } finally {
      unlisten();
    }
  }

  async function handleCreateFolder(name: string) {
    await createApplicationDirectory(applicationId, joinPath(path, name));
    toastSuccess(t("applicationFilesTab.createdFolderToast", { name }));
    load(path);
  }

  async function handleCreateFile(name: string) {
    const filePath = joinPath(path, name);
    await writeApplicationFile(applicationId, filePath, []);
    toastSuccess(t("applicationFilesTab.createdFileToast", { name }));
    load(path);
    setOpenFile({ name, path: filePath, isDir: false, isSymlink: false, size: 0 });
  }

  async function handleConfirmDelete() {
    if (!deletingEntry) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      await deleteApplicationFile(applicationId, deletingEntry.path);
      setDeletingEntry(null);
      load(path);
    } catch (err) {
      setDeleteError(errorMessage(err, t));
    } finally {
      setDeleteBusy(false);
    }
  }

  async function handleExtract(entry: RemoteFileEntry) {
    setExtractingPath(entry.path);
    setActionError(null);
    try {
      const count = await extractApplicationArchive(applicationId, entry.path, path);
      toastSuccess(t("applicationFilesTab.extractedToast", { count }));
      load(path);
    } catch (err) {
      setActionError(errorMessage(err, t));
    } finally {
      setExtractingPath(null);
    }
  }

  async function openQuickFile(quickFile: KnownFile) {
    setActionError(null);
    try {
      const entry = await getApplicationFileMetadata(applicationId, quickFile.path);
      setOpenFile(entry);
    } catch (err) {
      setActionError(errorMessage(err, t));
    }
  }

  /**
   * The row's callbacks, pinned.
   *
   * Every handler below closes over state this component re-renders on, so
   * handing them straight to a memoised row would defeat the memo on the
   * first poll. Reading them through a ref keeps the identity the row sees
   * constant while the behaviour behind it stays current - the usual shape
   * for an event callback that must not take part in memoisation.
   */
  const latest = useRef({ load, setOpenFile, runDownload, buildMenuItems, openContextMenu: contextMenu.open });
  latest.current = { load, setOpenFile, runDownload, buildMenuItems, openContextMenu: contextMenu.open };

  const handleOpenEntry = useCallback((entry: RemoteFileEntry) => {
    if (entry.isDir) latest.current.load(entry.path);
    else latest.current.setOpenFile(entry);
  }, []);
  const handleDownloadEntry = useCallback((entry: RemoteFileEntry) => void latest.current.runDownload(entry), []);
  const buildRowMenuItems = useCallback((entry: RemoteFileEntry) => latest.current.buildMenuItems(entry), []);
  const handleRowContextMenu = useCallback(
    (event: ReactMouseEvent, entry: RemoteFileEntry) => latest.current.openContextMenu(event, latest.current.buildMenuItems(entry)),
    [],
  );

  /** Shared by the per-row "..." button and right-click - same actions
   * either way, just two different ways to reach them (the global
   * native-context-menu suppression in main.tsx means right-click would
   * otherwise open nothing at all here). */
  function buildMenuItems(entry: RemoteFileEntry): ContextMenuItem[] {
    return [
      ...(!entry.isDir && /\.zip$/i.test(entry.name)
        ? [{ label: t("applicationFilesTab.extractAria", { name: entry.name }), icon: "archive", disabled: extractingPath === entry.path, onClick: () => handleExtract(entry) }]
        : []),
      { label: t("applicationFilesTab.renameAria", { name: entry.name }), icon: "edit", onClick: () => setRenameTarget({ entry, mode: "rename" }) },
      { label: t("applicationFilesTab.moveAria", { name: entry.name }), icon: "move", onClick: () => setRenameTarget({ entry, mode: "move" }) },
      { label: t("applicationFilesTab.copyAria", { name: entry.name }), icon: "copy", onClick: () => setRenameTarget({ entry, mode: "copy" }) },
      ...(entry.permissions !== undefined
        ? [{ label: t("applicationFilesTab.chmodAria", { name: entry.name }), icon: "lock", onClick: () => setChmodTarget(entry) }]
        : []),
      {
        label: t("applicationFilesTab.deleteAria", { name: entry.name }),
        icon: "trash",
        danger: true,
        onClick: () => {
          setDeleteError(null);
          setDeletingEntry(entry);
        },
      },
    ];
  }

  if (openFile) {
    return (
      <ApplicationFileEditorPanel
        applicationId={applicationId}
        entry={openFile}
        onClose={() => setOpenFile(null)}
        onSaved={() => load(path)}
      />
    );
  }

  return (
    <div className="application-detail-overview">
      {knownFiles.length > 0 && (
        <div className="application-files-quick">
          <span className="application-files-quick-label">{t("applicationFilesTab.quickFiles")}</span>
          {knownFiles.map((quickFile) => (
            <button key={quickFile.path} className="application-files-quick-button" onClick={() => openQuickFile(quickFile)}>
              {quickFile.label}
            </button>
          ))}
        </div>
      )}

      <div className="application-files-breadcrumb-row">
        <Breadcrumbs segments={segments} onNavigate={load} rootPath={ROOT_PATH} />
        <div className="application-files-toolbar">
          <GuideLink topic="application-files" />
          <Button variant="secondary" size="sm" onClick={() => setCreateModal("folder")}>
            <Icon name="folder-plus" size={14} />
            {t("applicationFilesTab.newFolder")}
          </Button>
          <Button variant="secondary" size="sm" onClick={() => setCreateModal("file")}>
            <Icon name="file-plus" size={14} />
            {t("applicationFilesTab.newFile")}
          </Button>
          <Button size="sm" onClick={handleUpload}>
            <Icon name="upload" size={14} />
            {t("applicationFilesTab.upload")}
          </Button>
        </div>
      </div>

      {error && <p className="page-error-note">{error}</p>}

      <Card className={`application-files-card ${dragging ? "application-files-card-dropping" : ""}`.trim()}>
        {/* The drop target is the whole card rather than the list, so a drop
            still lands when the directory is empty and the list is an empty
            state instead of rows. */}
        {dragging && (
          <div className="application-files-drop" aria-hidden="true">
            <Icon name="upload" size={28} />
            <p className="application-files-drop-title">{t("applicationFilesTab.dropTitle")}</p>
            <p className="application-files-drop-path">{path === ROOT_PATH ? "/" : `/${path}`}</p>
          </div>
        )}
        {loading ? (
          <SkeletonRows />
        ) : entries.length === 0 ? (
          <EmptyState icon="folder" title={t("applicationFilesTab.emptyTitle")} description={t("applicationFilesTab.emptyDescription")} />
        ) : (
          <>
          <div className="files-selection-bar">
            <input
              className="files-filter-input"
              type="search"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder={t("filesPage.filterPlaceholder")}
              aria-label={t("filesPage.filterPlaceholder")}
            />
          </div>
          {visibleEntries.length === 0 ? (
            <EmptyState icon="search" title={t("filesPage.noMatchesTitle")} description={t("filesPage.noMatchesDescription")} />
          ) : (
          <ul className="server-list">
            {visibleEntries.map((entry) => (
              <FileRow
                key={entry.path}
                entry={entry}
                language={i18n.language}
                onOpen={handleOpenEntry}
                onDownload={handleDownloadEntry}
                onContextMenu={handleRowContextMenu}
                buildMenuItems={buildRowMenuItems}
              />
            ))}
          </ul>
          )}
          {truncated && (
            <p className="form-note">{t("filesPage.showingFirst", { shown: visibleEntries.length, total: matchingEntries.length })}</p>
          )}
          </>
        )}
      </Card>

      {contextMenu.element}

      <TransferQueuePanel />

      {createModal && (
        <CreateEntryModal
          mode={createModal}
          onClose={() => setCreateModal(null)}
          onCreate={createModal === "folder" ? handleCreateFolder : handleCreateFile}
        />
      )}

      {renameTarget && (
        <RenameOrMoveModal
          mode={renameTarget.mode}
          currentPath={renameTarget.entry.path}
          currentName={renameTarget.entry.name}
          onClose={() => setRenameTarget(null)}
          onConfirm={async (to) => {
            if (renameTarget.mode === "copy") {
              await copyApplicationFile(applicationId, renameTarget.entry.path, to);
            } else {
              await renameApplicationFile(applicationId, renameTarget.entry.path, to);
            }
            load(path);
          }}
        />
      )}

      {chmodTarget && (
        <ChmodModal
          fileName={chmodTarget.name}
          currentMode={chmodTarget.permissions}
          onClose={() => setChmodTarget(null)}
          onConfirm={async (mode) => {
            await setApplicationFilePermissions(applicationId, chmodTarget.path, mode);
            load(path);
          }}
        />
      )}

      {jarWarning && (
        <JarReplaceWarningModal
          fileName={jarWarning.fileName}
          isRunning={isRunning}
          onCancel={() => setJarWarning(null)}
          onUploadOnly={handleJarUploadOnly}
          onUploadAndRestart={handleJarUploadAndRestart}
        />
      )}

      {deletingEntry && (
        <div className="modal-backdrop" {...deleteBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...deleteBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="applicationfilestab-dialog-title-1">{t("applicationFilesTab.deleteTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setDeletingEntry(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">{t("applicationFilesTab.deleteBody", { name: deletingEntry.name })}</p>
              {deleteError && <p className="form-note form-note-danger form-note-spaced">{deleteError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setDeletingEntry(null)} disabled={deleteBusy}>
                  {t("common.cancel")}
                </Button>
                <Button variant="danger" onClick={handleConfirmDelete} disabled={deleteBusy}>
                  {t("common.remove")}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatOctal(mode: number): string {
  return (mode & 0o777).toString(8).padStart(3, "0");
}
