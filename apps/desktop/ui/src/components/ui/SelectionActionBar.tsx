import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { IconButton } from "@/components/ui/IconButton";
import "./SelectionActionBar.css";

/**
 * What can be done with the current selection, pinned to the bottom of the
 * view while anything is selected.
 *
 * It lived in the toolbar above the list, so selecting rows further down -
 * the usual case, a run of old backups at the bottom of a folder - meant
 * scrolling back up to act on them. Sticky rather than fixed, so it stays
 * centred on the list it belongs to instead of on the whole window, and
 * settles under the last row when the list is short.
 */
export function SelectionActionBar({ count, onClear, children }: { count: number; onClear: () => void; children: ReactNode }) {
  const { t } = useTranslation();
  if (count === 0) return null;
  return (
    <div className="selection-action-bar-dock">
      <div className="selection-action-bar" role="region" aria-label={t("filesPage.selectionActions")}>
        <span className="selection-action-bar-count">{t("filesPage.selectedCount", { count })}</span>
        <div className="selection-action-bar-actions">{children}</div>
        <IconButton icon="x" size="sm" onClick={onClear} title={t("filesPage.clearSelectionAria")} />
      </div>
    </div>
  );
}
