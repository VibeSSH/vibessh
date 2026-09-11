import { FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Checkbox } from "@/components/ui/Checkbox";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import {
  cloudCreateRole,
  cloudDeleteRole,
  cloudListPermissions,
  cloudListRoles,
  cloudUpdateRole,
} from "@/services/cloudService";
import { toastSuccess } from "@/stores/toastStore";
import type { CloudRoleWithPermissions } from "@/types/cloud";
import { groupPermissions } from "./permissionCatalog";
import "./RolesSection.css";
import { errorMessage } from "@/services/tauri";

interface RoleFormState {
  id: string | null; // null = creating
  name: string;
  description: string;
  permissions: Set<string>;
}

function emptyForm(): RoleFormState {
  return { id: null, name: "", description: "", permissions: new Set() };
}

type Translate = (key: string, opts?: Record<string, unknown>) => string;

/** Permission keys are the backend's stable catalog strings (e.g. "team.roles.manage") - not meant to be read directly, so every one needs an entry under roles.permissionLabels in each locale file (falls back to the raw key if a new permission ships before its translation does). */
function permissionLabel(t: Translate, permission: string): string {
  return t(`roles.permissionLabels.${permission}`, { defaultValue: permission });
}

/** A sentence saying what the permission actually lets somebody do. Optional:
 * a permission with no hint yet simply has no tooltip. */
function permissionHint(t: Translate, permission: string): string | undefined {
  return t(`roles.permissionHints.${permission}`, { defaultValue: "" }) || undefined;
}

/**
 * A built-in role's name and description, in the reader's language.
 *
 * The backend writes them once, in English, as a row created with the team -
 * so the stored text is an identifier as much as it is prose, and showing it
 * verbatim is what put "Full control over the team..." in a Polish
 * interface. Translated by that stable name, falling back to what is stored
 * for anything this app has not been taught.
 */
function roleName(t: Translate, role: CloudRoleWithPermissions): string {
  return role.isSystem ? t(`roles.systemNames.${role.name}`, { defaultValue: role.name }) : role.name;
}

function roleDescription(t: Translate, role: CloudRoleWithPermissions): string {
  if (!role.isSystem) return role.description ?? "";
  return t(`roles.systemDescriptions.${role.name}`, { defaultValue: role.description ?? "" });
}

export function RolesSection({ teamId, canManage }: { teamId: string; canManage: boolean }) {
  const { t } = useTranslation();
  const [roles, setRoles] = useState<CloudRoleWithPermissions[]>([]);
  const [allPermissions, setAllPermissions] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<RoleFormState | null>(null);
  const [saving, setSaving] = useState(false);

  function load() {
    setLoading(true);
    setError(null);
    Promise.all([cloudListRoles(teamId), cloudListPermissions()])
      .then(([loadedRoles, loadedPermissions]) => {
        setRoles(loadedRoles);
        setAllPermissions(loadedPermissions);
      })
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }

  useEffect(load, [teamId]); // eslint-disable-line react-hooks/exhaustive-deps

  function togglePermission(permission: string) {
    setForm((prev) => {
      if (!prev) return prev;
      const next = new Set(prev.permissions);
      if (next.has(permission)) next.delete(permission);
      else next.add(permission);
      return { ...prev, permissions: next };
    });
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!form) return;
    const name = form.name.trim();
    if (!name) return;
    setSaving(true);
    setError(null);
    try {
      const permissions = Array.from(form.permissions);
      const description = form.description.trim() || null;
      if (form.id) {
        await cloudUpdateRole(teamId, form.id, name, description, permissions);
        toastSuccess(t("roles.updatedToast", { name }));
      } else {
        await cloudCreateRole(teamId, name, description, permissions);
        toastSuccess(t("roles.createdToast", { name }));
      }
      setForm(null);
      load();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(role: CloudRoleWithPermissions) {
    setError(null);
    try {
      await cloudDeleteRole(teamId, role.id);
      toastSuccess(t("roles.deletedToast", { name: role.name }));
      load();
    } catch (err) {
      setError(errorMessage(err, t));
    }
  }

  return (
    <Card title={t("roles.title")} subtitle={t("roles.subtitle")}>
      {error && <p className="page-error-note">{error}</p>}

      {loading ? (
        <SkeletonRows />
      ) : roles.length === 0 ? (
        <EmptyState icon="key" title={t("roles.emptyTitle")} description={t("roles.emptyDescription")} />
      ) : (
        <ul className="roles-list">
          {roles.map((role) => (
            <li key={role.id} className="roles-list-item">
              <div className="roles-list-main">
                <div className="roles-list-name-row">
                  <span className="roles-list-name" title={roleName(t, role)}>
                    {roleName(t, role)}
                  </span>
                  {role.isSystem && <Badge tone="neutral">{t("roles.builtIn")}</Badge>}
                  <span className="roles-list-count">
                    {role.permissions.length === 0
                      ? t("roles.noPermissions")
                      : t("roles.permissionsChosen", { count: role.permissions.length })}
                  </span>
                </div>
                {roleDescription(t, role) && <p className="roles-list-description">{roleDescription(t, role)}</p>}
                <div className="roles-permission-chips">
                  {role.permissions.map((permission) => (
                    <span key={permission} className="roles-permission-chip" title={permissionHint(t, permission)}>
                      {permissionLabel(t, permission)}
                    </span>
                  ))}
                </div>
              </div>
              {!role.isSystem && canManage && (
                <div className="roles-list-actions">
                  <IconButton
                    icon="edit"
                    size="sm"
                    title={t("roles.editAria", { name: role.name })}
                    onClick={() => setForm({ id: role.id, name: role.name, description: role.description ?? "", permissions: new Set(role.permissions) })}
                  />
                  <IconButton icon="trash" size="sm" danger title={t("roles.deleteAria", { name: role.name })} onClick={() => handleDelete(role)} />
                </div>
              )}
            </li>
          ))}
        </ul>
      )}

      {!canManage ? null : form ? (
        <form className="roles-form" onSubmit={handleSubmit}>
          {/* Which of the two things this form is doing. Without it, editing
              a role and creating one looked identical, and the only clue was
              whether the fields happened to be filled in. */}
          <p className="roles-form-title">
            {form.id === null ? t("roles.createTitle") : t("roles.editTitle", { name: form.name || t("roles.namePlaceholder") })}
          </p>
          <label className="form-field">
            <span className="form-label">{t("roles.name")}</span>
            <input
              className="form-input"
              value={form.name}
              onChange={(e) => setForm((prev) => (prev ? { ...prev, name: e.target.value } : prev))}
              placeholder={t("roles.namePlaceholder")}
              required
            />
          </label>
          <label className="form-field">
            <span className="form-label">{t("roles.description")}</span>
            <input
              className="form-input"
              value={form.description}
              onChange={(e) => setForm((prev) => (prev ? { ...prev, description: e.target.value } : prev))}
              placeholder={t("roles.descriptionPlaceholder")}
            />
          </label>
          <div className="form-field">
            <span className="form-label">{t("roles.permissions")}</span>
            {/* Grouped by what each permission acts on. A single undivided
                grid of them is a list to read; four short groups is a set of
                decisions to make. */}
            {/* Said where the granting happens, not only in the guide. A
                permission list implies enforcement, and for the Applications
                and Node groups that implication would be wrong - those
                operations run over each member's own SSH connection and
                never reach the backend. Somebody handing out roles has to
                read this before they believe a checkbox protects a Node. */}
            <div className="roles-guard-rail-note">
              <Icon name="alert-triangle" size={14} />
              <div>
                <p className="roles-guard-rail-title">{t("roles.guardRailTitle")}</p>
                <p className="roles-guard-rail-body">{t("roles.guardRailBody")}</p>
              </div>
            </div>
            {groupPermissions(allPermissions).map(({ group, permissions }) => (
              <div key={group} className="roles-permission-group">
                <p className="roles-permission-group-title">{t(`roles.groups.${group}`, { defaultValue: group })}</p>
                <div className="roles-permission-grid">
                  {permissions.map((permission) => (
                    <div key={permission} className="roles-permission-option">
                      <Checkbox
                        checked={form.permissions.has(permission)}
                        onChange={() => togglePermission(permission)}
                        label={permissionLabel(t, permission)}
                      />
                      {permissionHint(t, permission) && <p className="roles-permission-hint">{permissionHint(t, permission)}</p>}
                    </div>
                  ))}
                </div>
              </div>
            ))}
            <p className="form-note roles-permission-summary">{t("roles.permissionsChosen", { count: form.permissions.size })}</p>
          </div>
          <div className="form-actions">
            <Button type="button" variant="secondary" onClick={() => setForm(null)} disabled={saving}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={saving || !form.name.trim()}>
              {saving ? t("common.loading") : t("common.save")}
            </Button>
          </div>
        </form>
      ) : (
        <div className="roles-create-row">
          <Button variant="secondary" onClick={() => setForm(emptyForm())}>
            <Icon name="plus" size={14} />
            {t("roles.create")}
          </Button>
        </div>
      )}
    </Card>
  );
}
