import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { IconButton } from "@/components/ui/IconButton";
import { cloudAssignRole, cloudListMemberRoles, cloudListRoles, cloudUnassignRole } from "@/services/cloudService";
import { toastError } from "@/stores/toastStore";
import type { CloudRole } from "@/types/cloud";
import "./MemberRolesEditor.css";
import { errorMessage } from "@/services/tauri";

interface MemberRolesEditorProps {
  teamId: string;
  userId: string;
  memberName: string;
  isOwner: boolean;
  canManage: boolean;
}

/** A member's role assignment is edited from a small popover on their own row - not a separate page, since this is a quick toggle action, not something with enough of its own state to deserve navigation. */
export function MemberRolesEditor({ teamId, userId, memberName, isOwner, canManage }: MemberRolesEditorProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [allRoles, setAllRoles] = useState<CloudRole[]>([]);
  const [assignedRoleIds, setAssignedRoleIds] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);
  const [busyRoleId, setBusyRoleId] = useState<string | null>(null);
  const anchorRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function handlePointerDown(event: MouseEvent) {
      if (anchorRef.current && !anchorRef.current.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", handlePointerDown);
    return () => document.removeEventListener("mousedown", handlePointerDown);
  }, [open]);

  if (!canManage) return null;

  function load() {
    setLoading(true);
    Promise.all([cloudListRoles(teamId), cloudListMemberRoles(teamId, userId)])
      .then(([roles, memberRoles]) => {
        setAllRoles(roles);
        setAssignedRoleIds(new Set(memberRoles.map((r) => r.id)));
      })
      .catch((err) => toastError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }

  function handleOpen() {
    setOpen((o) => !o);
    if (!open) load();
  }

  async function toggle(role: CloudRole) {
    setBusyRoleId(role.id);
    try {
      if (assignedRoleIds.has(role.id)) {
        await cloudUnassignRole(teamId, userId, role.id);
        setAssignedRoleIds((prev) => {
          const next = new Set(prev);
          next.delete(role.id);
          return next;
        });
      } else {
        await cloudAssignRole(teamId, userId, role.id);
        setAssignedRoleIds((prev) => new Set(prev).add(role.id));
      }
    } catch (err) {
      toastError(errorMessage(err, t));
    } finally {
      setBusyRoleId(null);
    }
  }

  return (
    <div className="member-roles-anchor" ref={anchorRef}>
      <IconButton icon="key" size="sm" title={t("roles.manageRolesAria", { name: memberName })} onClick={handleOpen} />
      {open && (
        <div className="member-roles-popover">
          <div className="member-roles-popover-header">{t("roles.rolesFor", { name: memberName })}</div>
          {loading ? (
            <p className="rail-popover-empty">{t("common.loading")}</p>
          ) : allRoles.length === 0 ? (
            <p className="rail-popover-empty">{t("roles.emptyTitle")}</p>
          ) : (
            <ul className="member-roles-list">
              {allRoles.map((role) => {
                const assigned = assignedRoleIds.has(role.id);
                const lockedOwnerRole = isOwner && role.isSystem;
                return (
                  <li key={role.id}>
                    <label className={`member-roles-checkbox ${lockedOwnerRole ? "member-roles-checkbox-locked" : ""}`}>
                      <input
                        type="checkbox"
                        checked={assigned}
                        disabled={busyRoleId === role.id || lockedOwnerRole}
                        onChange={() => toggle(role)}
                      />
                      {role.name}
                    </label>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
