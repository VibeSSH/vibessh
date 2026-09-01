import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { pullApplicationImage, recreateApplication, refreshApplicationStatus, setApplicationImage } from "@/services/applicationService";
import { toastSuccess } from "@/stores/toastStore";
import type { ApplicationDetail, DockerImageConfig } from "@/types/application";
import "@/components/servers/forms.css";

interface DockerImageCardProps {
  applicationId: string;
  application: ApplicationDetail;
  onSaved: () => void;
}

/** Docker-only (see `application.runtimeType === "docker"` gating at the call site) - view/change/update the image a container runs, the design doc's "Docker image" requirement. Both actions patch `runtimeConfig` only; an already-running container keeps its old layers until a Recreate, same "change it, save it, it just works" auto-recreate pattern `ResourceLimitsCard`/`EnvironmentTab`/`PortsTab` already follow for their own saves. */
export function DockerImageCard({ applicationId, application, onSaved }: DockerImageCardProps) {
  const { t } = useTranslation();
  const config = (application.runtimeConfig ?? {}) as DockerImageConfig;
  const currentImage = config.image ?? "";

  const [editing, setEditing] = useState(false);
  const [image, setImage] = useState(currentImage);
  const [busy, setBusy] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Checks the freshly-probed status, not `application.status` - see
  // `EnvironmentTab`'s own `recreateIfRunningDocker` for why that prop
  // alone can still say "stopped" for a few seconds after a real start.
  async function recreateIfRunning() {
    const status = await refreshApplicationStatus(applicationId);
    if (status === "running") {
      await recreateApplication(applicationId);
    }
  }

  function startEditing() {
    setImage(currentImage);
    setError(null);
    setEditing(true);
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const trimmed = image.trim();
    if (!trimmed) {
      setError(t("dockerImage.invalidForm"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await setApplicationImage(applicationId, trimmed);
      setEditing(false);
      await recreateIfRunning();
      onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("dockerImage.saveError"));
    } finally {
      setBusy(false);
    }
  }

  async function handleUpdate() {
    setUpdating(true);
    setError(null);
    try {
      await pullApplicationImage(applicationId);
      await recreateIfRunning();
      toastSuccess(t("dockerImage.updateSuccessToast", { image: currentImage }));
      onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("dockerImage.updateError"));
    } finally {
      setUpdating(false);
    }
  }

  return (
    <Card title={t("dockerImage.title")}>
      <p className="form-note">{t("dockerImage.dockerNote")}</p>

      {editing ? (
        <form className="server-form" onSubmit={handleSubmit}>
          {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
          <label className="form-field">
            <span className="form-label">{t("dockerImage.imageLabel")}</span>
            <input className="form-input" value={image} onChange={(e) => setImage(e.target.value)} placeholder={t("dockerImage.imagePlaceholder")} autoFocus />
          </label>
          <div className="form-actions">
            <Button type="button" variant="secondary" onClick={() => setEditing(false)} disabled={busy}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={busy}>
              {busy ? t("common.saving") : t("common.save")}
            </Button>
          </div>
        </form>
      ) : (
        <>
          {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
          <div className="wizard-review-grid">
            <span className="wizard-review-label">{t("dockerImage.currentImage")}</span>
            <span className="wizard-review-value">{currentImage || "-"}</span>
          </div>
          <div className="form-actions">
            <Button variant="secondary" size="sm" onClick={handleUpdate} disabled={updating || !currentImage} title={t("dockerImage.updateAria", { image: currentImage })}>
              <Icon name="refresh-cw" size={14} />
              {updating ? t("dockerImage.updating") : t("dockerImage.update")}
            </Button>
            <Button variant="secondary" size="sm" onClick={startEditing} disabled={updating}>
              <Icon name="edit" size={14} />
              {t("dockerImage.change")}
            </Button>
          </div>
        </>
      )}
    </Card>
  );
}
