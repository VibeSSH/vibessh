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
import { Checkbox } from "@/components/ui/Checkbox";
import { fileIcon } from "@/utils/fileIcons";
import { Card } from "@/components/ui/Card";
import { useContextMenu, type ContextMenuItem } from "@/components/ui/ContextMenu";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { SelectionActionBar } from "@/components/ui/SelectionActionBar";
import { CompressModal } from "@/components/files/CompressModal";
import { IconButton } from "@/components/ui/IconButton";
import { OverflowMenu } from "@/components/ui/OverflowMenu";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { CreateEntryModal } from "@/components/servers/CreateEntryModal";
import { useFileDrop } from "@/hooks/useFileDrop";
import { useModalDialog } from "@/hooks/useModalDialog";
import { restartApplication } from "@/services/applicationService";
import {
  compressApplicationFiles,
  copyApplicationFile,
  createApplicationDirectory,
  deleteApplicationFile,
  downloadApplicationFile,
  extractApplicationArchive,
  fetchApplicationFileUrl,
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
import { discardFileDraft, rememberFilesView, rememberedFilesView } from "@/stores/applicationFilesStore";
import { ApplicationFileEditorPanel } from "./ApplicationFileEditorPanel";
import { ChmodModal } from "./ChmodModal";
import { FetchUrlModal } from "./FetchUrlModal";
import { formatBytes } from "@/utils/formatBytes";
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
  selected: boolean;
  onToggleSelect: (path: string) => void;
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
 * not change it, and the callbacks are pinned in the parent. `selected` is a
 * boolean that changes only for the row being toggled, so it does not undo
 * that.
 */
const FileRow = memo(function FileRow({ entry, selected, onToggleSelect, language, onOpen, onDownload, onContextMenu, buildMenuItems }: FileRowProps) {
  const { t } = useTranslation();
  const icon = fileIcon(entry.name, entry.isDir);
  return (
    // A row of a table rather than a tile: size, date and permissions in
    // columns that line up with the header, so a folder of backups reads as a
    // list that can be compared and sorted instead of a stack of cards.
    <li className={`files-row ${selected ? "files-row-selected" : ""}`.trim()} onContextMenu={(event) => onContextMenu(event, entry)}>
      <Checkbox checked={selected} onChange={() => onToggleSelect(entry.path)} label={null} />
      <span className={`files-row-icon file-icon-${icon.tone}`}>
        <Icon name={icon.name} size={15} />
      </span>
      <button className="files-entry-name" title={entry.name} onClick={() => onOpen(entry)}>
        <span className="files-entry-name-text">{entry.name}</span>
        {entry.isSymlink && <Badge tone="neutral">{t("applicationFilesTab.symlink")}</Badge>}
      </button>
      <span className="files-row-cell files-row-end">{entry.isDir ? "—" : formatSize(entry.size)}</span>
      <span className="files-row-cell files-row-end">{entry.modifiedAt ? formatShortDate(entry.modifiedAt, language) : "—"}</span>
      <span className="files-row-cell files-row-end files-row-mono">{entry.permissions !== undefined ? formatOctal(entry.permissions) : "—"}</span>
      <span className="files-row-actions">
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
      </span>
    </li>
  );
});

const ROOT_PATH = ".";

type FileSortKey = "name" | "size" | "modified";
interface FileSort {
  key: FileSortKey;
  descending: boolean;
}

const NAME_ORDER = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/**
 * Folders first whatever the column, then the chosen order. Names compare
 * naturally - `world2` before `world10` - which is how people number them.
 */
function sortEntries(entries: RemoteFileEntry[], sort: FileSort): RemoteFileEntry[] {
  const direction = sort.descending ? -1 : 1;
  return [...entries].sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    let order = 0;
    if (sort.key === "size") order = a.size - b.size;
    else if (sort.key === "modified") order = Date.parse(a.modifiedAt ?? "") - Date.parse(b.modifiedAt ?? "") || 0;
    if (order === 0) order = NAME_ORDER.compare(a.name, b.name);
    return order * direction;
  });
}

/** A column title that sorts the listing, and shows which way it is sorted. */
function SortHeader({
  label,
  column,
  sort,
  onSort,
  align,
}: {
  label: string;
  column: FileSortKey;
  sort: FileSort;
  onSort: (column: FileSortKey) => void;
  align?: "end";
}) {
  const active = sort.key === column;
  return (
    <button
      type="button"
      className={`files-row-head-cell files-sort ${align === "end" ? "files-row-end" : ""} ${active ? "files-sort-active" : ""}`}
      onClick={() => onSort(column)}
      aria-pressed={active}
    >
      {label}
      {active && <Icon name={sort.descending ? "chevron-down" : "chevron-up"} size={12} />}
    </button>
  );
}

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

  // Where this Application's tab was left - see `applicationFilesStore`. The
  // page remounts on every switch between Applications, so without this a
  // trip to another server landed back at the top with the file closed.
  const [path, setPath] = useState(() => rememberedFilesView(applicationId)?.path ?? ROOT_PATH);
  const [filter, setFilter] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);
  const [openFile, setOpenFile] = useState<RemoteFileEntry | null>(() => rememberedFilesView(applicationId)?.openFile ?? null);
  useEffect(() => {
    rememberFilesView(applicationId, { path, openFile });
  }, [applicationId, path, openFile]);
  // Split-view editing: whether the open editor has unsaved changes (reported
  // up by the editor), and a file the user asked to switch to while it did -
  // held until they confirm discarding, so a click in the side list never
  // throws away edits silently.
  const [editorDirty, setEditorDirty] = useState(false);
  const [pendingSwitch, setPendingSwitch] = useState<RemoteFileEntry | null>(null);
  const [createModal, setCreateModal] = useState<"file" | "folder" | null>(null);
  const [fetchOpen, setFetchOpen] = useState(false);
  const [renameTarget, setRenameTarget] = useState<{ entry: RemoteFileEntry; mode: "rename" | "move" | "copy" } | null>(null);
  // Acting on a whole selection from the bar at the bottom of the list.
  const [compressTargets, setCompressTargets] = useState<RemoteFileEntry[] | null>(null);
  const [movingEntries, setMovingEntries] = useState<RemoteFileEntry[] | null>(null);
  const [chmodTarget, setChmodTarget] = useState<RemoteFileEntry | null>(null);
  const [deletingEntries, setDeletingEntries] = useState<RemoteFileEntry[] | null>(null);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [extractingPath, setExtractingPath] = useState<string | null>(null);
  const [jarWarning, setJarWarning] = useState<{ fileName: string; localSrc: string; targetPath: string } | null>(null);
  const deleteBackdrop = useModalDialog(() => !deleteBusy && setDeletingEntries(null), { labelledBy: "applicationfilestab-dialog-title-1" });
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
    // Against the global default, and only here. That default is off because
    // alt-tabbing back would otherwise fire every mounted query at once - a
    // burst of SSH channels for readings that are usually still fine. A
    // directory listing is the exception: the files are not this app's, and
    // the likeliest thing to have happened while the window was in the
    // background is that somebody changed them. One listing is also a cheap
    // question, unlike the dashboard-wide burst that default is guarding
    // against.
    refetchOnWindowFocus: true,
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
      // Cleared on the way out of a directory: a selection is a set of paths,
      // and carrying it into a listing where none of them appear would leave
      // "3 selected" over rows that are not the selected ones.
      setSelectedPaths(new Set());
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

  const [sort, setSort] = useState<FileSort>({ key: "name", descending: false });
  const matchingEntries = useMemo(() => {
    const needle = deferredFilter.trim().toLowerCase();
    const filtered = needle ? entries.filter((entry) => entry.name.toLowerCase().includes(needle)) : entries;
    return sortEntries(filtered, sort);
  }, [entries, deferredFilter, sort]);
  // A click on the column already sorted flips it; a new column starts in its
  // natural direction - names A to Z, the largest and the newest first.
  const toggleSort = (key: FileSortKey) =>
    setSort((current) => (current.key === key ? { key, descending: !current.descending } : { key, descending: key !== "name" }));
  const folderCount = matchingEntries.filter((entry) => entry.isDir).length;
  const fileCount = matchingEntries.length - folderCount;
  const visibleEntries = matchingEntries.slice(0, MAX_ROWS_SHOWN);
  const allSelected = matchingEntries.length > 0 && matchingEntries.every((entry) => selectedPaths.has(entry.path));
  const truncated = matchingEntries.length > visibleEntries.length;

  // To wherever this Application was left when the application changes - the
  // previous one's tree says nothing about this one.
  useEffect(() => {
    setPath(rememberedFilesView(applicationId)?.path ?? ROOT_PATH);
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
    if (!deletingEntries) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      // One at a time and in order, so a failure half way through leaves a
      // knowable state: everything before it is gone, everything after it is
      // not, and the message names what stopped it.
      for (const entry of deletingEntries) {
        await deleteApplicationFile(applicationId, entry.path);
      }
      setDeletingEntries(null);
      setSelectedPaths(new Set());
      load(path);
    } catch (err) {
      setDeleteError(errorMessage(err, t));
      load(path);
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

  /** Pinned, like the other row callbacks - a new identity every render would
      re-reconcile all two hundred rows on every poll. */
  const toggleSelected = useCallback((entryPath: string) => {
    setSelectedPaths((previous) => {
      const next = new Set(previous);
      if (!next.delete(entryPath)) next.add(entryPath);
      return next;
    });
  }, []);
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
          setDeletingEntries([entry]);
        },
      },
    ];
  }

  // Open a file in the editor beside the list. A different file requested with
  // unsaved changes in the current one is held for a discard confirmation
  // rather than switched under the edits.
  function requestOpenFile(entry: RemoteFileEntry) {
    if (openFile && entry.path === openFile.path) return;
    if (editorDirty) {
      setPendingSwitch(entry);
      return;
    }
    setEditorDirty(false);
    setOpenFile(entry);
  }

  function closeEditor() {
    setEditorDirty(false);
    setOpenFile(null);
  }

  if (openFile) {
    const dirs = visibleEntries.filter((entry) => entry.isDir);
    const files = visibleEntries.filter((entry) => !entry.isDir);
    return (
      <div className="application-files-editor-split">
        <div className="application-files-editor-main">
          <ApplicationFileEditorPanel
            key={openFile.path}
            applicationId={applicationId}
            entry={openFile}
            onClose={closeEditor}
            onSaved={() => load(path)}
            onDirtyChange={setEditorDirty}
          />
        </div>

        <aside className="application-files-editor-sidebar">
          <div className="application-files-editor-sidebar-head">
            <Breadcrumbs segments={segments} onNavigate={load} rootPath={ROOT_PATH} />
          </div>
          <div className="application-files-editor-tree" data-lenis-prevent>
            {loading ? (
              <p className="form-note application-files-editor-tree-note">{t("applicationFileEditor.loading")}</p>
            ) : visibleEntries.length === 0 ? (
              <p className="form-note application-files-editor-tree-note">{t("applicationFilesTab.emptyTitle")}</p>
            ) : (
              <ul className="application-files-editor-tree-list">
                {dirs.map((entry) => {
                  const icon = fileIcon(entry.name, true);
                  return (
                    <li key={entry.path}>
                      <button className="application-files-editor-tree-row" onClick={() => load(entry.path)} title={entry.name}>
                        <span className={`application-files-editor-tree-icon file-icon-${icon.tone}`}>
                          <Icon name={icon.name} size={15} />
                        </span>
                        <span className="application-files-editor-tree-name">{entry.name}</span>
                      </button>
                    </li>
                  );
                })}
                {files.map((entry) => {
                  const icon = fileIcon(entry.name, false);
                  const active = entry.path === openFile.path;
                  return (
                    <li key={entry.path}>
                      <button
                        className={`application-files-editor-tree-row ${active ? "application-files-editor-tree-row-active" : ""}`.trim()}
                        onClick={() => requestOpenFile(entry)}
                        title={entry.name}
                        aria-current={active ? "true" : undefined}
                      >
                        <span className={`application-files-editor-tree-icon file-icon-${icon.tone}`}>
                          <Icon name={icon.name} size={15} />
                        </span>
                        <span className="application-files-editor-tree-name">{entry.name}</span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </aside>

        {pendingSwitch && (
          <SwitchFileDialog
            fileName={openFile.name}
            onCancel={() => setPendingSwitch(null)}
            onDiscard={() => {
              const next = pendingSwitch;
              discardFileDraft(applicationId, openFile.path);
              setPendingSwitch(null);
              setEditorDirty(false);
              setOpenFile(next);
            }}
          />
        )}
      </div>
    );
  }

  return (
    <div className="application-detail-overview">
      {error && <p className="page-error-note">{error}</p>}

      {/* One card for the whole browser: where you are, what you can do
          here, and the listing. The quick files, the breadcrumb and the
          buttons used to be three separate strips stacked above it. */}
      <Card
        title={t("applicationDetail.tabFiles")}
        subtitle={path === ROOT_PATH ? application.workingDirectory : `${application.workingDirectory}/${path}`}
        className={`application-files-card ${dragging ? "application-files-card-dropping" : ""}`.trim()}
        actions={
          <>
            <GuideLink topic="application-files" />
            <Button variant="secondary" size="sm" onClick={() => setCreateModal("folder")}>
              <Icon name="folder-plus" size={14} />
              {t("applicationFilesTab.newFolder")}
            </Button>
            <Button variant="secondary" size="sm" onClick={() => setCreateModal("file")}>
              <Icon name="file-plus" size={14} />
              {t("applicationFilesTab.newFile")}
            </Button>
            <Button variant="secondary" size="sm" onClick={() => setFetchOpen(true)}>
              <Icon name="download" size={14} />
              {t("applicationFilesTab.fetchButton")}
            </Button>
            <Button size="sm" onClick={handleUpload}>
              <Icon name="upload" size={14} />
              {t("applicationFilesTab.upload")}
            </Button>
          </>
        }
      >
        <div className="application-files-nav">
          <IconButton
            icon="chevron-up"
            size="sm"
            title={t("applicationFilesTab.upOneLevel")}
            onClick={() => load(segments.length > 1 ? segments.slice(0, -1).join("/") : ROOT_PATH)}
            disabled={path === ROOT_PATH}
          />
          <Breadcrumbs segments={segments} onNavigate={load} rootPath={ROOT_PATH} />
        </div>

        {knownFiles.length > 0 && (
          <div className="application-files-quick">
            <span className="application-files-quick-label">{t("applicationFilesTab.quickFiles")}</span>
            {knownFiles.map((quickFile) => (
              <button key={quickFile.path} className="application-files-quick-button" onClick={() => openQuickFile(quickFile)}>
                <Icon name="file" size={12} />
                {quickFile.label}
              </button>
            ))}
          </div>
        )}

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
        ) : entries.length === 0 && loadError ? (
          // Not "this folder is empty": nothing was read, and saying empty
          // reads as the files having gone. The reason is in the banner above.
          <EmptyState icon="alert-triangle" title={t("applicationFilesTab.loadFailedTitle")} description={t("applicationFilesTab.loadFailedDescription")} />
        ) : entries.length === 0 ? (
          <EmptyState icon="folder" title={t("applicationFilesTab.emptyTitle")} description={t("applicationFilesTab.emptyDescription")} />
        ) : (
          <>
          <div className="files-selection-bar">
            {/* Selects what the filter matches, not only the rows drawn: the
                cap below is a limit on rendering, not on what "all" means,
                and the count says how many that is. */}
            <Checkbox
              checked={allSelected}
              onChange={(checked) => setSelectedPaths(checked ? new Set(matchingEntries.map((entry) => entry.path)) : new Set())}
              label={t("filesPage.selectAll")}
            />
            <input
              className="files-filter-input"
              type="search"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder={t("filesPage.filterPlaceholder")}
              aria-label={t("filesPage.filterPlaceholder")}
            />
            <span className="application-files-summary">{t("applicationFilesTab.summary", { folders: folderCount, files: fileCount })}</span>
          </div>
          {visibleEntries.length === 0 ? (
            <EmptyState icon="search" title={t("filesPage.noMatchesTitle")} description={t("filesPage.noMatchesDescription")} />
          ) : (
          <>
          <div className="files-row files-row-head" role="presentation">
            <span />
            <span />
            <SortHeader label={t("applicationFilesTab.columnName")} column="name" sort={sort} onSort={toggleSort} />
            <SortHeader label={t("applicationFilesTab.columnSize")} column="size" sort={sort} onSort={toggleSort} align="end" />
            <SortHeader label={t("applicationFilesTab.columnModified")} column="modified" sort={sort} onSort={toggleSort} align="end" />
            <span className="files-row-head-cell files-row-end">{t("applicationFilesTab.columnPermissions")}</span>
            <span />
          </div>
          <ul className="files-rows">
            {visibleEntries.map((entry) => (
              <FileRow
                key={entry.path}
                entry={entry}
                selected={selectedPaths.has(entry.path)}
                onToggleSelect={toggleSelected}
                language={i18n.language}
                onOpen={handleOpenEntry}
                onDownload={handleDownloadEntry}
                onContextMenu={handleRowContextMenu}
                buildMenuItems={buildRowMenuItems}
              />
            ))}
          </ul>
          </>
          )}
          {truncated && (
            <p className="form-note">{t("filesPage.showingFirst", { shown: visibleEntries.length, total: matchingEntries.length })}</p>
          )}
          <SelectionActionBar count={selectedPaths.size} onClear={() => setSelectedPaths(new Set())}>
            <Button variant="secondary" size="sm" onClick={() => setCompressTargets(matchingEntries.filter((entry) => selectedPaths.has(entry.path)))}>
              <Icon name="archive" size={14} />
              {t("filesPage.compressSelectedShort")}
            </Button>
            <Button variant="secondary" size="sm" onClick={() => setMovingEntries(matchingEntries.filter((entry) => selectedPaths.has(entry.path)))}>
              <Icon name="move" size={14} />
              {t("filesPage.moveSelectedShort")}
            </Button>
            <Button
              variant="danger"
              size="sm"
              onClick={() => setDeletingEntries(matchingEntries.filter((entry) => selectedPaths.has(entry.path)))}
            >
              <Icon name="trash" size={14} />
              {t("filesPage.deleteSelectedShort")}
            </Button>
          </SelectionActionBar>
          </>
        )}
      </Card>

      {contextMenu.element}

      <TransferQueuePanel />

      {fetchOpen && (
        <FetchUrlModal
          folderLabel={path === ROOT_PATH ? "/" : `/${path}`}
          onClose={() => setFetchOpen(false)}
          onFetch={async (url, fileName) => {
            const size = await fetchApplicationFileUrl(applicationId, joinPath(path, fileName), url);
            setFetchOpen(false);
            toastSuccess(t("applicationFilesTab.fetchedToast", { name: fileName, size: formatBytes(size) }));
            load(path);
          }}
        />
      )}

      {createModal && (
        <CreateEntryModal
          mode={createModal}
          onClose={() => setCreateModal(null)}
          onCreate={createModal === "folder" ? handleCreateFolder : handleCreateFile}
        />
      )}

      {compressTargets && (
        <CompressModal
          targets={compressTargets}
          onClose={() => setCompressTargets(null)}
          onConfirm={async (archiveName) => {
            await compressApplicationFiles(
              applicationId,
              compressTargets.map((entry) => entry.path),
              joinPath(path, archiveName),
            );
            toastSuccess(t("filesPage.compressedToast", { name: archiveName }));
            setSelectedPaths(new Set());
            load(path);
          }}
        />
      )}

      {movingEntries && (
        <RenameOrMoveModal
          mode="move"
          currentPath={path}
          currentName=""
          destinationHelp={{
            note: t("applicationFilesTab.moveManyNote", { count: movingEntries.length }),
            placeholder: t("applicationFilesTab.moveManyPlaceholder"),
          }}
          onClose={() => setMovingEntries(null)}
          onConfirm={async (to) => {
            // One at a time, and every failure kept: a move that stops at
            // the first error leaves the rest silently where they were.
            const directory = to.replace(/\/+$/, "") || ROOT_PATH;
            const failed: string[] = [];
            for (const entry of movingEntries) {
              try {
                await renameApplicationFile(applicationId, entry.path, joinPath(directory, entry.name));
              } catch (err) {
                failed.push(`${entry.name}: ${errorMessage(err, t)}`);
              }
            }
            setSelectedPaths(new Set());
            load(path);
            if (failed.length > 0) {
              throw new Error(t("applicationFilesTab.moveManyFailed", { count: failed.length, details: failed.join("; ") }));
            }
            toastSuccess(t("applicationFilesTab.movedManyToast", { count: movingEntries.length }));
          }}
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

      {deletingEntries && (
        <div className="modal-backdrop" {...deleteBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...deleteBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="applicationfilestab-dialog-title-1">{t("applicationFilesTab.deleteTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setDeletingEntries(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">
                {deletingEntries.length === 1
                  ? t("applicationFilesTab.deleteBody", { name: deletingEntries[0].name })
                  : t("applicationFilesTab.deleteManyBody", { count: deletingEntries.length })}
              </p>
              {deleteError && <p className="form-note form-note-danger form-note-spaced">{deleteError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setDeletingEntries(null)} disabled={deleteBusy}>
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

/** Stands between a click on another file in the side list and losing the
 *  edits in the one open now - the split-view counterpart to the editor's own
 *  back-button discard guard. */
function SwitchFileDialog({ fileName, onCancel, onDiscard }: { fileName: string; onCancel: () => void; onDiscard: () => void }) {
  const { t } = useTranslation();
  const dialog = useModalDialog(onCancel, { labelledBy: "switch-file-title" });
  return (
    <div className="modal-backdrop" {...dialog.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...dialog.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="switch-file-title">
            {t("applicationFileEditor.discardTitle")}
          </h2>
          <IconButton icon="x" size="sm" onClick={onCancel} title={t("common.close")} />
        </div>
        <div className="modal-body">
          <p className="dialog-body-text">{t("applicationFilesTab.switchDiscardBody", { name: fileName })}</p>
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

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatOctal(mode: number): string {
  return (mode & 0o777).toString(8).padStart(3, "0");
}
