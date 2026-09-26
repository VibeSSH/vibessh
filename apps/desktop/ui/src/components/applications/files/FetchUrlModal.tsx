import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { errorMessage } from "@/services/tauri";

/**
 * The file name a link most likely wants - its last path segment, decoded.
 * Empty when the link has none (a bare domain), and the person types one.
 */
export function fileNameFromUrl(link: string): string {
  try {
    const segment = new URL(link.trim()).pathname.split("/").filter(Boolean).pop() ?? "";
    const name = decodeURIComponent(segment);
    // A name, not a path: anything that could step out of the folder goes.
    return name.replace(/[\\/]/g, "").replace(/^\.+/, "");
  } catch {
    return "";
  }
}

/**
 * "Download from a link": the Node fetches the file into the folder being
 * viewed, so a plugin from Modrinth never makes the trip through this
 * computer. The name follows the link until the person types their own.
 */
export function FetchUrlModal({
  folderLabel,
  onClose,
  onFetch,
}: {
  /** Where it lands, as shown to the person - `/plugins`. */
  folderLabel: string;
  onClose: () => void;
  /** Resolves once the file is in place; throws with the reason it failed. */
  onFetch: (url: string, fileName: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [url, setUrl] = useState("");
  const [fileName, setFileName] = useState("");
  const [nameEdited, setNameEdited] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const name = nameEdited ? fileName : fileNameFromUrl(url);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await onFetch(url.trim(), name.trim());
    } catch (err) {
      setError(errorMessage(err, t));
      setBusy(false);
    }
  }

  return (
    <Dialog open onClose={onClose} size="sm" dismissable={!busy} title={t("applicationFilesTab.fetchTitle")}>
      <form className="modal-body server-form" onSubmit={submit}>
        <label className="form-field">
          <span className="form-label">{t("applicationFilesTab.fetchUrl")}</span>
          <input
            className="form-input"
            type="url"
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            placeholder="https://cdn.modrinth.com/data/…/plugin.jar"
            disabled={busy}
            autoFocus
            required
          />
        </label>
        <label className="form-field">
          <span className="form-label">{t("applicationFilesTab.fetchName")}</span>
          <input
            className="form-input"
            value={name}
            onChange={(event) => {
              setNameEdited(true);
              setFileName(event.target.value);
            }}
            disabled={busy}
            required
          />
          <span className="form-note">{t("applicationFilesTab.fetchWhere", { folder: folderLabel })}</span>
        </label>
        <p className="form-note">{busy ? t("applicationFilesTab.fetchBusy") : t("applicationFilesTab.fetchNote")}</p>
        {error && <p className="form-note form-note-danger">{error}</p>}
        <div className="form-actions">
          <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" disabled={busy || !url.trim() || !name.trim()}>
            {busy ? t("applicationFilesTab.fetching") : t("applicationFilesTab.fetchConfirm")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
