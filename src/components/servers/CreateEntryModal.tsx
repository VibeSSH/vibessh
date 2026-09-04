import { FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Select } from "@/components/ui/Select";
import { IconButton } from "@/components/ui/IconButton";
import { useModalDialog } from "@/hooks/useModalDialog";
import "./AddServerModal.css";
import "./forms.css";

type Mode = "file" | "folder";

/** Extensions this app's CodeMirror setup actually highlights (see editorLanguage.ts) - the dropdown only offers formats the editor can tell apart, rather than a decorative list that doesn't affect anything once the file is opened. */
const FILE_FORMATS: { extension: string; label: string }[] = [
  { extension: "", label: "Plain Text" },
  { extension: ".json", label: "JSON" },
  { extension: ".yaml", label: "YAML" },
  { extension: ".js", label: "JavaScript" },
  { extension: ".ts", label: "TypeScript" },
  { extension: ".py", label: "Python" },
  { extension: ".md", label: "Markdown" },
  { extension: ".css", label: "CSS" },
  { extension: ".html", label: "HTML" },
  { extension: ".xml", label: "XML" },
  { extension: ".sql", label: "SQL" },
  { extension: ".toml", label: "TOML" },
  { extension: ".sh", label: "Shell" },
  { extension: ".conf", label: "Config (INI)" },
];

interface CreateEntryModalProps {
  mode: Mode;
  onClose: () => void;
  onCreate: (name: string) => Promise<void>;
}

export function CreateEntryModal({ mode, onClose, onCreate }: CreateEntryModalProps) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [format, setFormat] = useState(FILE_FORMATS[0].extension);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function resolvedName(): string {
    const trimmed = name.trim();
    if (mode === "folder" || !format || !trimmed) return trimmed;
    return trimmed.toLowerCase().endsWith(format) ? trimmed : `${trimmed}${format}`;
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    const finalName = resolvedName();
    if (!finalName) return;
    setSaving(true);
    setError(null);
    try {
      await onCreate(finalName);
      onClose();
    } catch (err) {
      const fallback = mode === "folder" ? t("filesPage.couldntCreateFolder") : t("filesPage.couldntCreateFile");
      setError(err instanceof Error ? err.message : fallback);
    } finally {
      setSaving(false);
    }
  }

  const backdrop = useModalDialog(onClose, { labelledBy: "createentrymodal-dialog-title-1" });

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="createentrymodal-dialog-title-1">{mode === "folder" ? t("filesPage.newFolderTitle") : t("filesPage.newFileTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>

        <div className="modal-body">
          <form className="server-form" onSubmit={handleSubmit}>
            {error && <p className="page-error-note">{error}</p>}

            <label className="form-field">
              <span className="form-label">{mode === "folder" ? t("filesPage.folderName") : t("filesPage.fileName")}</span>
              <input
                className="form-input"
                autoFocus
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={mode === "folder" ? t("filesPage.folderNamePlaceholder") : t("filesPage.fileNamePlaceholder")}
                required
              />
            </label>

            {mode === "file" && (
              <label className="form-field">
                <span className="form-label">{t("filesPage.format")}</span>
                <Select value={format} onChange={setFormat} items={FILE_FORMATS.map((f) => ({ value: f.extension, label: f.label }))} />
              </label>
            )}

            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={saving}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={saving || !name.trim()}>
                {saving ? t("common.loading") : t("common.create")}
              </Button>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
}
