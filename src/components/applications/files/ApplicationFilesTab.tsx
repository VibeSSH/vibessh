import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Badge } from "@/components/ui/Badge";
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
import { useBackdropClose } from "@/hooks/useBackdropClose";
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
import { errorMessage } from "@/services/tauri";

/// Matches the Node Files page and the Actions page.
const MAX_ROWS_SHOWN = 200;

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
  const { t } = useTranslation();
  const currentJarName = extractCurrentJarName(application.runtimeConfig);
  const isRunning = application.status === "running";

  const [path, setPath] = useState(ROOT_PATH);
  const [filter, setFilter] = useState("");
  const [entries, setEntries] = useState<RemoteFileEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [openFile, setOpenFile] = useState<RemoteFileEntry | null>(null);
  const [createModal, setCreateModal] = useState<"file" | "folder" | null>(null);
  const [renameTarget, setRenameTarget] = useState<{ entry: RemoteFileEntry; mode: "rename" | "move" | "copy" } | null>(null);
  const [chmodTarget, setChmodTarget] = useState<RemoteFileEntry | null>(null);
  const [deletingEntry, setDeletingEntry] = useState<RemoteFileEntry | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [extractingPath, setExtractingPath] = useState<string | null>(null);
  const [jarWarning, setJarWarning] = useState<{ fileName: string; localSrc: string; targetPath: string } | null>(null);
  const deleteBackdrop = useBackdropClose(() => !deleteBusy && setDeletingEntry(null));
  const contextMenu = useContextMenu();

  // Same reasoning as the Node Files page: a directory can hold tens of
  // thousands of entries and rendering a row each locks the window up, but
  // a bare cap would make distant entries unreachable. Cap plus filter.
  const matchingEntries = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return entries;
    return entries.filter((entry) => entry.name.toLowerCase().includes(needle));
  }, [entries, filter]);
  const visibleEntries = matchingEntries.slice(0, MAX_ROWS_SHOWN);
  const truncated = matchingEntries.length > visibleEntries.length;

  const addTransfer = useFileTransferStore((s) => s.addTransfer);
  const updateProgress = useFileTransferStore((s) => s.updateProgress);
  const markDone = useFileTransferStore((s) => s.markDone);
  const markError = useFileTransferStore((s) => s.markError);

  const load = useCallback(
    (targetPath: string) => {
      setLoading(true);
      setError(null);
      listApplicationFiles(applicationId, targetPath)
        .then((loaded) => {
          setEntries(loaded);
          setPath(targetPath);
        })
        .catch((err) => setError(errorMessage(err, t)))
        .finally(() => setLoading(false));
    },
    [applicationId, t],
  );

  useEffect(() => {
    load(ROOT_PATH);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [applicationId]);

  const segments = path === ROOT_PATH ? [] : path.split("/").filter(Boolean);

  async function runUpload(localSrc: string, targetPath: string) {
    const transferId = crypto.randomUUID();
    const fileName = targetPath.split("/").pop() ?? targetPath;
    addTransfer({ id: transferId, name: fileName, direction: "upload", total: 0, retry: () => runUpload(localSrc, targetPath) });
    const unlisten = await onTransferProgress(transferId, ({ transferred, total }) => updateProgress(transferId, transferred, total));
    try {
      await uploadApplicationFile(applicationId, localSrc, targetPath, transferId);
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
    const paths = Array.isArray(selected) ? selected : [selected];

    if (paths.length === 1) {
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
    for (const localSrc of paths) {
      const fileName = localSrc.split(/[/\\]/).pop() ?? localSrc;
      runUpload(localSrc, joinPath(path, fileName));
    }
  }

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
    setError(null);
    try {
      const count = await extractApplicationArchive(applicationId, entry.path, path);
      toastSuccess(t("applicationFilesTab.extractedToast", { count }));
      load(path);
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setExtractingPath(null);
    }
  }

  async function openQuickFile(quickFile: KnownFile) {
    setError(null);
    try {
      const entry = await getApplicationFileMetadata(applicationId, quickFile.path);
      setOpenFile(entry);
    } catch (err) {
      setError(errorMessage(err, t));
    }
  }

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

      <Card>
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
              <li key={entry.path} className="server-list-item" onContextMenu={(e) => contextMenu.open(e, buildMenuItems(entry))}>
                <div className="server-list-icon">
                  <Icon name={entry.isDir ? "folder" : "file"} size={16} />
                </div>
                <button
                  className="files-entry-name"
                  title={entry.name}
                  onClick={() => (entry.isDir ? load(entry.path) : setOpenFile(entry))}
                >
                  {entry.name}
                  {entry.isSymlink && <Badge tone="neutral">{t("applicationFilesTab.symlink")}</Badge>}
                </button>
                <div className="application-files-entry-meta">
                  {!entry.isDir && <span>{formatSize(entry.size)}</span>}
                  {entry.modifiedAt && <span>{new Date(entry.modifiedAt).toLocaleDateString()}</span>}
                  {entry.permissions !== undefined && <span>{formatOctal(entry.permissions)}</span>}
                </div>
                {!entry.isDir && (
                  <IconButton
                    icon="download"
                    size="sm"
                    title={t("applicationFilesTab.downloadAria", { name: entry.name })}
                    onClick={() => runDownload(entry)}
                  />
                )}
                <OverflowMenu ariaLabel={t("applicationFilesTab.moreAria", { name: entry.name })} items={buildMenuItems(entry)} />
              </li>
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
        <div className="modal-backdrop" {...deleteBackdrop}>
          <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">{t("applicationFilesTab.deleteTitle")}</h2>
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
