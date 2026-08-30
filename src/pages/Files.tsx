import { useCallback, useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { CreateEntryModal } from "@/components/servers/CreateEntryModal";
import { FileEditorPanel } from "@/components/servers/FileEditorPanel";
import { createRemoteDirectory, downloadRemoteFile, listRemoteDirectory, uploadRemoteFile, writeRemoteFile } from "@/services/filesService";
import { useServersStore } from "@/stores/serversStore";
import { toastError, toastSuccess } from "@/stores/toastStore";
import type { RemoteFileEntry } from "@/types/files";
import "./pages.css";
import "./Servers.css";
import "./Files.css";

const ROOT_PATH = ".";

/** Mirrors the backend's own path-joining rule (see ssh/sftp.rs's opendir prefix) - "." is the SFTP cwd, so a name under it needs no dot-prefix, just like breadcrumb targets already carry none. */
function joinRemotePath(dir: string, name: string): string {
  return dir === ROOT_PATH ? name : `${dir}/${name}`;
}

export function FilesPage() {
  const { t } = useTranslation();
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));

  const [path, setPath] = useState(ROOT_PATH);
  const [entries, setEntries] = useState<RemoteFileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [openFile, setOpenFile] = useState<RemoteFileEntry | null>(null);
  const [uploading, setUploading] = useState(false);
  const [downloadingPath, setDownloadingPath] = useState<string | null>(null);
  const [createModal, setCreateModal] = useState<"file" | "folder" | null>(null);

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

  const segments = path === ROOT_PATH ? [] : path.replace(/^\.\/?/, "").split("/").filter(Boolean);

  if (openFile) {
    return (
      <div className="page files-editor-page">
        <FileEditorPanel serverId={serverId} entry={openFile} onClose={() => setOpenFile(null)} />
      </div>
    );
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

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{server ? server.name : "Files"}</h1>
          <p className="page-subtitle">{server ? server.host : serverId}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/servers")}>
          <Icon name="chevron-left" size={16} />
          {t("common.backToServers")}
        </Button>
      </div>

      <div className="files-breadcrumb files-breadcrumb-row">
        <div>
          <button className="files-breadcrumb-item" onClick={() => load(ROOT_PATH)}>
            /
          </button>
          {segments.map((segment, index) => {
            const target = segments.slice(0, index + 1).join("/");
            return (
              <span key={target}>
                <span className="files-breadcrumb-sep">/</span>
                <button className="files-breadcrumb-item" onClick={() => load(target)}>
                  {segment}
                </button>
              </span>
            );
          })}
        </div>
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
          <ul className="server-list">
            {entries.map((entry) => (
              <li key={entry.path} className="server-list-item">
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
                    <button
                      className="server-list-action"
                      aria-label={t("filesPage.downloadAria", { name: entry.name })}
                      disabled={downloadingPath === entry.path}
                      onClick={() => handleDownload(entry)}
                    >
                      <Icon name="download" size={14} />
                    </button>
                  </>
                )}
              </li>
            ))}
          </ul>
        )}
      </Card>

      {createModal && (
        <CreateEntryModal
          mode={createModal}
          onClose={() => setCreateModal(null)}
          onCreate={createModal === "folder" ? handleCreateFolder : handleCreateFile}
        />
      )}
    </div>
  );
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
