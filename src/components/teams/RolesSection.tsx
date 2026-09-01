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

/** Permission keys are the backend's stable catalog strings (e.g. "team.roles.manage") - not meant to be read directly, so every one needs an entry under roles.permissionLabels in each locale file (falls back to the raw key if a new permission ships before its translation does). */
function permissionLabel(t: (key: string, opts?: Record<string, unknown>) => string, permission: string): string {
  return t(`roles.permissionLabels.${permission}`, { defaultValue: permission });
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
                  <span className="roles-list-name" title={role.name}>{role.name}</span>
                  {role.isSystem && <Badge tone="neutral">{t("roles.builtIn")}</Badge>}
                </div>
                {role.description && <p className="roles-list-description">{role.description}</p>}
                <div className="roles-permission-chips">
                  {role.permissions.map((permission) => (
                    <span key={permission} className="roles-permission-chip">
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
            <div className="roles-permission-grid">
              {allPermissions.map((permission) => (
                <Checkbox
                  key={permission}
                  checked={form.permissions.has(permission)}
                  onChange={() => togglePermission(permission)}
                  label={permissionLabel(t, permission)}
                />
              ))}
            </div>
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
