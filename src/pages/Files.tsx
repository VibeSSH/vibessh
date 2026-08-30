import { useCallback, useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { FileEditorPanel } from "@/components/servers/FileEditorPanel";
import { listRemoteDirectory } from "@/services/filesService";
import { useServersStore } from "@/stores/serversStore";
import type { RemoteFileEntry } from "@/types/files";
import "./pages.css";
import "./Servers.css";
import "./Files.css";

const ROOT_PATH = ".";

export function FilesPage() {
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));

  const [path, setPath] = useState(ROOT_PATH);
  const [entries, setEntries] = useState<RemoteFileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [openFile, setOpenFile] = useState<RemoteFileEntry | null>(null);

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
        .catch((err) => setError(err instanceof Error ? err.message : "Couldn't list this directory."))
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

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{server ? server.name : "Files"}</h1>
          <p className="page-subtitle">{server ? server.host : serverId}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/servers")}>
          <Icon name="chevron-left" size={16} />
          Back to servers
        </Button>
      </div>

      <div className="files-breadcrumb">
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

      {error && <p className="page-error-note">{error}</p>}

      <Card>
        {loading ? (
          <p className="settings-muted">Loading...</p>
        ) : entries.length === 0 ? (
          <EmptyState icon="folder" title="Empty directory" description="Nothing here." />
        ) : (
          <ul className="server-list">
            {entries.map((entry) => (
              <li key={entry.path} className="server-list-item">
                <div className="server-list-icon">
                  <Icon name={entry.isDir ? "folder" : "file"} size={16} />
                </div>
                <button
                  className="files-entry-name"
                  onClick={() => (entry.isDir ? load(entry.path) : setOpenFile(entry))}
                >
                  {entry.name}
                </button>
                {!entry.isDir && <span className="files-entry-size">{formatSize(entry.size)}</span>}
              </li>
            ))}
          </ul>
        )}
      </Card>

      {openFile && <FileEditorPanel serverId={serverId} entry={openFile} onClose={() => setOpenFile(null)} />}
    </div>
  );
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
