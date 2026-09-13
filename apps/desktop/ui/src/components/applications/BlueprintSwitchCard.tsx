import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useBlueprintText } from "./blueprintText";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { Select } from "@/components/ui/Select";
import { changeApplicationBlueprint, listBlueprints } from "@/services/applicationService";
import { errorMessage } from "@/services/tauri";
import { toastSuccess } from "@/stores/toastStore";
import type { ApplicationDetail, Blueprint } from "@/types/application";
import { BlueprintFieldInput, fieldValueOrDefault } from "./CreateApplicationWizard";
import "@/components/servers/forms.css";

interface BlueprintSwitchCardProps {
  applicationId: string;
  application: ApplicationDetail;
  current: Blueprint | null;
  onChanged: () => void;
}

/**
 * Moves an Application between a managed blueprint and a plain container.
 *
 * **Why both directions.** Adopting servers off a migrated host creates plain
 * Docker Applications on purpose - adoption must not replace a jar somebody
 * is already running. But a plain container has no Paper version to bump and
 * no `server.properties` known-file, so somebody who wants VibeSSH to manage
 * that server has to be able to say so afterwards. And somebody who tried a
 * managed blueprint and would rather keep their own jar has to be able to
 * back out. One control, both ways.
 *
 * **The two directions are not symmetrical, and the warning says which is
 * which.** Taking management over provisions, and provisioning downloads that
 * server's own jar into the working directory. Giving management up touches
 * nothing on disk: the files stay and the jar that is there keeps running.
 */
export function BlueprintSwitchCard({ applicationId, application, current, onChanged }: BlueprintSwitchCardProps) {
  const { t } = useTranslation();
  const blueprintText = useBlueprintText();
  const [blueprints, setBlueprints] = useState<Blueprint[]>([]);
  const [targetId, setTargetId] = useState("");
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listBlueprints()
      .then(setBlueprints)
      .catch(() => undefined);
  }, []);

  // Only blueprints that can run the way this Application already runs. The
  // backend refuses the others anyway - offering one would be offering a
  // dead end, and the runtime type is not something this changes.
  const options = blueprints.filter(
    (blueprint) => blueprint.id !== application.blueprintId && blueprint.supportedRuntimeTypes.includes(application.runtimeType),
  );
  const target = options.find((blueprint) => blueprint.id === targetId) ?? null;

  /**
   * Whether moving there downloads a server jar.
   *
   * Read off the field type rather than a list of blueprint names: a
   * `papermcVersion` field is by definition a version this blueprint fetches
   * a build for, so a blueprint that has one is a blueprint that provisions,
   * including any added later.
   */
  const willDownload = target?.fields.some((field) => field.fieldType === "papermcVersion") ?? false;

  function pick(id: string) {
    setTargetId(id);
    setError(null);
    // The new blueprint's own defaults, filled in ready to be edited. The old
    // blueprint's answers are deliberately not carried over - an `image` means
    // nothing to Paper, and the backend drops them for the same reason.
    const blueprint = options.find((option) => option.id === id);
    setValues(blueprint ? Object.fromEntries(blueprint.fields.map((field) => [field.key, fieldValueOrDefault(field, {})])) : {});
  }

  async function handleChange() {
    if (!target || busy) return;
    setBusy(true);
    setError(null);
    try {
      await changeApplicationBlueprint(applicationId, target.id, values);
      toastSuccess(t("blueprintSwitch.changedToast", { name: target.name }));
      setTargetId("");
      setValues({});
      onChanged();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  if (options.length === 0) return null;

  return (
    <Card title={t("blueprintSwitch.title")}>
      <p className="form-note">{t("blueprintSwitch.intro", { current: current?.name ?? application.blueprintId })}</p>

      <div className="server-form">
        <label className="form-field">
          <span className="form-label">{t("blueprintSwitch.target")}</span>
          <Select
            value={targetId}
            onChange={pick}
            placeholder={t("blueprintSwitch.pick")}
            items={options.map((blueprint) => ({ value: blueprint.id, label: blueprintText.name(blueprint) }))}
          />
        </label>

        {/* The new blueprint's questions, asked before the switch rather than
            after it. Paper cannot be provisioned without knowing which
            version to fetch, so defaulting them silently would either pick a
            version nobody chose or fail on a required field. */}
        {target?.fields.map((field) => (
          <BlueprintFieldInput
            key={field.key}
            field={field}
            value={values[field.key]}
            onChange={(value) => setValues((previous) => ({ ...previous, [field.key]: value }))}
            serverId={application.serverId ?? null}
          />
        ))}

        {/* Said before the button, and only for the direction that causes it. */}
        {target && (
          <p className={`form-note form-note-spaced ${willDownload ? "form-note-danger" : ""}`}>
            {willDownload
              ? t("blueprintSwitch.warnDownload", { name: target.name, directory: application.workingDirectory })
              : t("blueprintSwitch.warnPlain", { name: target.name })}
          </p>
        )}

        {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}

        <div className="form-actions">
          <Button onClick={() => void handleChange()} disabled={!target || busy}>
            <Icon name="refresh-cw" size={14} />
            {busy ? t("blueprintSwitch.changing") : t("blueprintSwitch.change")}
          </Button>
        </div>
      </div>
    </Card>
  );
}
