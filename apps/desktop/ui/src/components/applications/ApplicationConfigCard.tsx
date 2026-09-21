import { Fragment, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { BlueprintFieldInput, fieldValueOrDefault, formatFieldValueForReview } from "./CreateApplicationWizard";
import { updateApplicationConfig } from "@/services/applicationService";
import { useContainerApply } from "@/hooks/useContainerApply";
import { toastSuccess } from "@/stores/toastStore";
import type { ApplicationDetail, Blueprint } from "@/types/application";
import "@/components/servers/forms.css";
import "@/components/applications/CreateApplicationWizard.css";
import { errorMessage } from "@/services/tauri";

interface ApplicationConfigCardProps {
  applicationId: string;
  application: ApplicationDetail;
  blueprint: Blueprint | null;
  onApplied: (updated: ApplicationDetail) => void;
}

function storedBlueprintInputs(application: ApplicationDetail): Record<string, unknown> {
  const metadata = application.metadata;
  if (!metadata || typeof metadata !== "object") return {};
  const inputs = (metadata as Record<string, unknown>).blueprintInputs;
  return inputs && typeof inputs === "object" ? (inputs as Record<string, unknown>) : {};
}

/**
 * Lets JVM args, Java version, and every other blueprint-declared field
 * (Paper/Velocity version, ...) be changed after creation, not just once in
 * the wizard - reuses the exact same `BlueprintFieldInput` the wizard uses,
 * since these are the same fields with the same per-type editing UI.
 * `blueprintInputs` is stored on `application.metadata` starting with this
 * feature; an application created before it exists has none, so the first
 * edit shows the blueprint's own defaults rather than guessing at whatever
 * was actually typed into the wizard originally (see the Rust
 * `update_application_config`'s own doc comment for why a value can't be
 * safely reverse-engineered out of the already-rendered runtime config).
 */
export function ApplicationConfigCard({ applicationId, application, blueprint, onApplied }: ApplicationConfigCardProps) {
  const { t } = useTranslation();
  const applyToContainer = useContainerApply();
  const [editing, setEditing] = useState(false);
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!blueprint || blueprint.fields.length === 0) return null;

  const stored = storedBlueprintInputs(application);
  const isLegacy = Object.keys(stored).length === 0;
  // A blueprint's own supported runtime types can narrow after this
  // Application already exists on one that's no longer listed (Paper/
  // Velocity going Docker-only, for one) - it keeps running fine on its
  // already-stored config, but re-rendering that config from today's
  // blueprint logic isn't safe (see the Rust `update_application_config`'s
  // own doc comment), so editing is disabled here rather than failing only
  // after the user fills the form back in and hits save.
  const runtimeTypeUnsupported = !blueprint.supportedRuntimeTypes.includes(application.runtimeType);

  function startEditing() {
    const initial: Record<string, unknown> = {};
    for (const field of blueprint!.fields) {
      initial[field.key] = fieldValueOrDefault(field, stored);
    }
    setValues(initial);
    setError(null);
    setEditing(true);
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      // The config write returns the fresh application and is quick; show it
      // and settle the form at once. A Docker container bakes its launch
      // command in at `docker create` time, so the change only takes effect
      // once the container is recreated - but that recreate (the slow part)
      // now runs in the background via `useContainerApply` rather than
      // freezing this form on it. A stopped app is left stopped.
      const updated = await updateApplicationConfig(applicationId, values);
      setEditing(false);
      onApplied(updated);
      toastSuccess(t("applicationConfig.savedToast"));
      void applyToContainer(updated);
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title={t("applicationConfig.title")}>
      {runtimeTypeUnsupported ? (
        <p className="form-note">{t("applicationConfig.runtimeTypeUnsupportedNote", { blueprint: blueprint.name })}</p>
      ) : (
        isLegacy && !editing && <p className="form-note">{t("applicationConfig.legacyNote")}</p>
      )}

      {editing ? (
        <form className="server-form" onSubmit={handleSubmit}>
          {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
          {isLegacy && <p className="form-note form-note-spaced">{t("applicationConfig.legacyEditNote")}</p>}
          {blueprint.fields.map((field) => (
            <BlueprintFieldInput
              key={field.key}
              field={field}
              value={values[field.key]}
              onChange={(v) => setValues((prev) => ({ ...prev, [field.key]: v }))}
              serverId={application.serverId ?? null}
            />
          ))}
          <p className="form-note">
            {application.runtimeType === "docker"
              ? application.status === "running"
                ? t("applicationConfig.recreateAutoNote")
                : t("applicationConfig.recreateStoppedNote")
              : t("applicationConfig.restartNote")}
          </p>
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
          <div className="wizard-review-grid">
            {blueprint.fields.map((field) => (
              <Fragment key={field.key}>
                <span className="wizard-review-label">{field.label}</span>
                <span className="wizard-review-value">
                  {formatFieldValueForReview(field, fieldValueOrDefault(field, stored), t("common.yes"), t("common.no"))}
                </span>
              </Fragment>
            ))}
          </div>
          {!runtimeTypeUnsupported && (
            <div className="form-actions">
              <Button variant="secondary" size="sm" onClick={startEditing}>
                <Icon name="edit" size={14} />
                {t("applicationConfig.edit")}
              </Button>
            </div>
          )}
        </>
      )}
    </Card>
  );
}
