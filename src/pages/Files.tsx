import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Breadcrumbs } from "@/components/ui/Breadcrumbs";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Checkbox } from "@/components/ui/Checkbox";
import { useContextMenu, type ContextMenuItem } from "@/components/ui/ContextMenu";
import { EmptyState } from "@/components/ui/EmptyState";
import { HostAddress } from "@/components/ui/HostAddress";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { CreateEntryModal } from "@/components/servers/CreateEntryModal";
import { FileEditorPanel } from "@/components/servers/FileEditorPanel";
import { RenameOrMoveModal } from "@/components/applications/files/RenameOrMoveModal";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import {
  compressRemotePaths,
  createRemoteDirectory,
  deleteRemotePath,
  downloadRemoteFile,
  extractRemoteArchive,
  listRemoteDirectory,
  renameRemotePath,
  uploadRemoteFile,
  writeRemoteFile,
} from "@/services/filesService";
import { useServersStore } from "@/stores/serversStore";
import { toastError, toastSuccess } from "@/stores/toastStore";
import type { RemoteFileEntry } from "@/types/files";
import "./pages.css";
import "./Servers.css";
import "./Files.css";

/** The real filesystem root, not the SFTP login user's home directory - every OpenSSH server understands an absolute path here the same way, so this is what a plain SFTP client would show first (var/lib/root/... siblings visible immediately, not just reachable by navigating up from wherever the account happens to land). */
/// Matches the cap the Actions page already uses. Large enough that an
/// ordinary directory is never truncated, small enough that a pathological
/// one stays responsive.
const MAX_ROWS_SHOWN = 200;

const ROOT_PATH = "/";

/** Mirrors the backend's own path-joining rule (see ssh/sftp.rs's `list_directory`, whose `entry.path()` is root-relative the same way) - joining directly under "/" needs the slash itself as the only separator, everywhere else it's "dir/name" like normal. */
function joinRemotePath(dir: string, name: string): string {
  return dir === ROOT_PATH ? `/${name}` : `${dir}/${name}`;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function FilesPage() {
  const { t } = useTranslation();
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));
  const contextMenu = useContextMenu();

  const [path, setPath] = useState(ROOT_PATH);
  const [entries, setEntries] = useState<RemoteFileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [openFile, setOpenFile] = useState<RemoteFileEntry | null>(null);
  const [uploading, setUploading] = useState(false);
  const [downloadingPath, setDownloadingPath] = useState<string | null>(null);
  const [createModal, setCreateModal] = useState<"file" | "folder" | null>(null);
  const [filter, setFilter] = useState("");
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [renameTarget, setRenameTarget] = useState<RemoteFileEntry | null>(null);
  const [moveTargets, setMoveTargets] = useState<RemoteFileEntry[] | null>(null);
  const [deletingEntries, setDeletingEntries] = useState<RemoteFileEntry[] | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [compressTargets, setCompressTargets] = useState<RemoteFileEntry[] | null>(null);
  const [extractingPath, setExtractingPath] = useState<string | null>(null);
  const deleteBackdrop = useBackdropClose(() => !deleteBusy && setDeletingEntries(null));

  const load = useCallback(
    (targetPath: string) => {
      if (!serverId) return;
      setLoading(true);
      setError(null);
      listRemoteDirectory(serverId, targetPath)
        .then((loaded) => {
          const sorted = [...loaded].sort(
            (a, b) => Number(b.isDir) - Number(a.isDir) || a.name.localeCompare(b.name),
          );
          setEntries(sorted);
          setPath(targetPath);
          setSelectedPaths(new Set());
        })
        .catch((err) => setError(err instanceof Error ? err.message : t("filesPage.couldntList")))
        .finally(() => setLoading(false));
    },
    [serverId],
  );

  useEffect(() => {
    load(ROOT_PATH);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId]);

  if (!serverId) {
    return <Navigate to="/servers" replace />;
  }

  const segments = path === ROOT_PATH ? [] : path.split("/").filter(Boolean);

  // A remote directory can hold tens of thousands of entries - a Minecraft
  // world's region folder routinely does - and rendering a row for each one
  // locks the window up for seconds. Capping the rendered rows fixes that,
  // but on its own it would make entry 5000 unreachable, so the cap comes
  // with a filter. Together they are more useful than virtualisation would
  // be here: finding a known filename by typing part of it beats scrolling
  // to it.
  const matchingEntries = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return entries;
    return entries.filter((entry) => entry.name.toLowerCase().includes(needle));
  }, [entries, filter]);
  const visibleEntries = matchingEntries.slice(0, MAX_ROWS_SHOWN);
  const truncated = matchingEntries.length > visibleEntries.length;

  if (openFile) {
    return (
      <div className="page files-editor-page">
        <FileEditorPanel serverId={serverId} entry={openFile} onClose={() => setOpenFile(null)} />
      </div>
    );
  }

  function toggleSelected(entryPath: string) {
    setSelectedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(entryPath)) next.delete(entryPath);
      else next.add(entryPath);
      return next;
    });
  }

  async function handleUpload() {
    if (!serverId) return;
    const localPath = await open({ multiple: false, title: "Upload file" });
    if (!localPath || Array.isArray(localPath)) return;
    const fileName = localPath.split(/[/\\]/).pop() ?? localPath;
    setUploading(true);
    try {
      await uploadRemoteFile(serverId, localPath, joinRemotePath(path, fileName));
      toastSuccess(t("filesPage.uploadedToast", { name: fileName }));
      load(path);
    } catch (err) {
      toastError(err instanceof Error ? err.message : t("filesPage.couldntUpload", { name: fileName }));
    } finally {
      setUploading(false);
    }
  }

  async function handleCreateFolder(name: string) {
    if (!serverId) return;
    await createRemoteDirectory(serverId, joinRemotePath(path, name));
    toastSuccess(t("filesPage.createdFolderToast", { name }));
    load(path);
  }

  async function handleCreateFile(name: string) {
    if (!serverId) return;
    const remotePath = joinRemotePath(path, name);
    await writeRemoteFile(serverId, remotePath, []);
    toastSuccess(t("filesPage.createdFileToast", { name }));
    load(path);
    setOpenFile({ name, path: remotePath, isDir: false, isSymlink: false, size: 0 });
  }

  async function handleDownload(entry: RemoteFileEntry) {
    if (!serverId) return;
    const localPath = await save({ defaultPath: entry.name, title: "Download file" });
    if (!localPath) return;
    setDownloadingPath(entry.path);
    try {
      await downloadRemoteFile(serverId, entry.path, localPath);
      toastSuccess(t("filesPage.downloadedToast", { name: entry.name }));
    } catch (err) {
      toastError(err instanceof Error ? err.message : t("filesPage.couldntDownload", { name: entry.name }));
    } finally {
      setDownloadingPath(null);
    }
  }

  async function handleExtract(entry: RemoteFileEntry) {
    if (!serverId) return;
    setExtractingPath(entry.path);
    try {
      const count = await extractRemoteArchive(serverId, entry.path, path);
      toastSuccess(t("filesPage.extractedToast", { count }));
      load(path);
    } catch (err) {
      toastError(err instanceof Error ? err.message : t("filesPage.extractError"));
    } finally {
      setExtractingPath(null);
    }
  }

  async function handleConfirmDelete() {
    if (!deletingEntries || !serverId) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      for (const entry of deletingEntries) {
        await deleteRemotePath(serverId, entry.path);
      }
      setDeletingEntries(null);
      load(path);
    } catch (err) {
      setDeleteError(err instanceof Error ? err.message : t("filesPage.deleteError"));
    } finally {
      setDeleteBusy(false);
    }
  }

  function buildMenuItems(targets: RemoteFileEntry[]): ContextMenuItem[] {
    const isSingle = targets.length === 1;
    const items: ContextMenuItem[] = [];
    if (isSingle && !targets[0].isDir) {
      items.push({ label: t("filesPage.downloadAria", { name: targets[0].name }), icon: "download", onClick: () => handleDownload(targets[0]) });
    }
    if (isSingle) {
      items.push({ label: t("filesPage.renameAria", { name: targets[0].name }), icon: "edit", onClick: () => setRenameTarget(targets[0]) });
    }
    items.push({
      label: isSingle ? t("filesPage.moveAria", { name: targets[0].name }) : t("filesPage.moveSelectedAria", { count: targets.length }),
      icon: "move",
      onClick: () => setMoveTargets(targets),
    });
    if (isSingle && !targets[0].isDir && /\.zip$/i.test(targets[0].name)) {
      items.push({
        label: t("filesPage.extractAria", { name: targets[0].name }),
        icon: "archive",
        disabled: extractingPath === targets[0].path,
        onClick: () => handleExtract(targets[0]),
      });
    }
    items.push({
      label: isSingle ? t("filesPage.compressAria", { name: targets[0].name }) : t("filesPage.compressSelectedAria", { count: targets.length }),
      icon: "archive",
      onClick: () => setCompressTargets(targets),
    });
    items.push({
      label: isSingle ? t("filesPage.deleteAria", { name: targets[0].name }) : t("filesPage.deleteSelectedAria", { count: targets.length }),
      icon: "trash",
      danger: true,
      onClick: () => setDeletingEntries(targets),
    });
    return items;
  }

  function handleRowContextMenu(e: React.MouseEvent, entry: RemoteFileEntry) {
    if (selectedPaths.has(entry.path) && selectedPaths.size > 1) {
      contextMenu.open(e, buildMenuItems(entries.filter((en) => selectedPaths.has(en.path))));
      return;
    }
    setSelectedPaths(new Set([entry.path]));
    contextMenu.open(e, buildMenuItems([entry]));
  }

  const allSelected = entries.length > 0 && selectedPaths.size === entries.length;

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{server ? server.name : "Files"}</h1>
          <p className="page-subtitle">{server ? <HostAddress value={server.host} /> : serverId}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/servers")}>
          <Icon name="chevron-left" size={16} />
          {t("common.backToServers")}
        </Button>
      </div>

      <div className="files-breadcrumb-row">
        <Breadcrumbs
          segments={segments}
          onNavigate={(target) => load(target === ROOT_PATH ? ROOT_PATH : `/${target}`)}
          rootPath={ROOT_PATH}
        />
        <div className="files-toolbar-actions">
          <Button variant="secondary" size="sm" onClick={() => setCreateModal("folder")}>
            <Icon name="folder-plus" size={14} />
            {t("filesPage.newFolder")}
          </Button>
          <Button variant="secondary" size="sm" onClick={() => setCreateModal("file")}>
            <Icon name="file-plus" size={14} />
            {t("filesPage.newFile")}
          </Button>
          <Button variant="secondary" size="sm" onClick={handleUpload} disabled={uploading}>
            <Icon name="upload" size={14} />
            {uploading ? t("filesPage.uploading") : t("filesPage.upload")}
          </Button>
        </div>
      </div>

      {error && <p className="page-error-note">{error}</p>}

      <Card>
        {loading ? (
          <SkeletonRows />
        ) : entries.length === 0 ? (
          <EmptyState icon="folder" title={t("filesPage.emptyTitle")} description={t("filesPage.emptyDescription")} />
        ) : (
          <>
            <div className="files-selection-bar">
              <Checkbox
                checked={allSelected}
                onChange={(checked) => setSelectedPaths(checked ? new Set(entries.map((en) => en.path)) : new Set())}
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
              {selectedPaths.size > 0 && <span className="files-selection-count">{t("filesPage.selectedCount", { count: selectedPaths.size })}</span>}
            </div>
            {visibleEntries.length === 0 ? (
              <EmptyState icon="search" title={t("filesPage.noMatchesTitle")} description={t("filesPage.noMatchesDescription")} />
            ) : (
            <ul className="server-list">
              {visibleEntries.map((entry) => (
                <li
                  key={entry.path}
                  className={`server-list-item ${selectedPaths.has(entry.path) ? "files-entry-selected" : ""}`}
                  onContextMenu={(e) => handleRowContextMenu(e, entry)}
                >
                  <Checkbox checked={selectedPaths.has(entry.path)} onChange={() => toggleSelected(entry.path)} label={null} />
                  <div className="server-list-icon">
                    <Icon name={entry.isDir ? "folder" : "file"} size={16} />
                  </div>
                  <button
                    className="files-entry-name"
                    title={entry.name}
                    onClick={() => (entry.isDir ? load(entry.path) : setOpenFile(entry))}
                  >
                    {entry.name}
                  </button>
                  {!entry.isDir && (
                    <>
                      <span className="files-entry-size">{formatSize(entry.size)}</span>
                      <IconButton
                        icon="download"
                        size="sm"
                        title={t("filesPage.downloadAria", { name: entry.name })}
                        disabled={downloadingPath === entry.path}
                        onClick={() => handleDownload(entry)}
                      />
                    </>
                  )}
                </li>
              ))}
            </ul>
            )}
            {truncated && (
              <p className="form-note">
                {t("filesPage.showingFirst", { shown: visibleEntries.length, total: matchingEntries.length })}
              </p>
            )}
          </>
        )}
      </Card>

      {contextMenu.element}

      {createModal && (
        <CreateEntryModal
          mode={createModal}
          onClose={() => setCreateModal(null)}
          onCreate={createModal === "folder" ? handleCreateFolder : handleCreateFile}
        />
      )}

      {renameTarget && (
        <RenameOrMoveModal
          mode="rename"
          currentPath={renameTarget.path}
          currentName={renameTarget.name}
          onClose={() => setRenameTarget(null)}
          onConfirm={async (to) => {
            await renameRemotePath(serverId, renameTarget.path, to);
            load(path);
          }}
        />
      )}

      {moveTargets && (
        <RenameOrMoveModal
          mode="move"
          currentPath={moveTargets[0].path}
          currentName={moveTargets[0].name}
          onClose={() => setMoveTargets(null)}
          onConfirm={async (to) => {
            const destinationDir = `/${to}`.replace(/\/+$/, "");
            for (const entry of moveTargets) {
              await renameRemotePath(serverId, entry.path, `${destinationDir}/${entry.name}`);
            }
            load(path);
          }}
          destinationHelp={{ note: t("filesPage.moveNote"), placeholder: t("filesPage.moveNotePlaceholder") }}
        />
      )}

      {compressTargets && (
        <CompressModal
          targets={compressTargets}
          onClose={() => setCompressTargets(null)}
          onConfirm={async (archiveName) => {
            await compressRemotePaths(serverId, compressTargets.map((entry) => entry.path), joinRemotePath(path, archiveName));
            toastSuccess(t("filesPage.compressedToast", { name: archiveName }));
            load(path);
          }}
        />
      )}

      {deletingEntries && (
        <div className="modal-backdrop" {...deleteBackdrop}>
          <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">{t("filesPage.deleteTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setDeletingEntries(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">
                {deletingEntries.length === 1
                  ? t("filesPage.deleteBody", { name: deletingEntries[0].name })
                  : t("filesPage.deleteBodyMulti", { count: deletingEntries.length })}
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

interface CompressModalProps {
  targets: RemoteFileEntry[];
  onClose: () => void;
  onConfirm: (archiveName: string) => Promise<void>;
}

/** Names the archive, then compresses `targets` into it inside the current directory - "spakuj" in the row/selection context menu. */
function CompressModal({ targets, onClose, onConfirm }: CompressModalProps) {
  const { t } = useTranslation();
  const backdrop = useBackdropClose(onClose);
  const defaultName = targets.length === 1 ? targets[0].name.replace(/\.[^./]+$/, "") : "archive";
  const [name, setName] = useState(defaultName);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) return;
    setBusy(true);
    setError(null);
    try {
      await onConfirm(trimmed.toLowerCase().endsWith(".zip") ? trimmed : `${trimmed}.zip`);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("filesPage.compressError"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop}>
      <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("filesPage.compressTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            <label className="form-field">
              <span className="form-label">{t("filesPage.archiveName")}</span>
              <input className="form-input" autoFocus value={name} onChange={(e) => setName(e.target.value)} />
            </label>
            <p className="form-note">{t("filesPage.compressNote", { count: targets.length })}</p>
            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={busy || !name.trim()}>
                {busy ? t("common.loading") : t("common.create")}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
