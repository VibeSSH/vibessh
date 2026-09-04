import * as RadixDialog from "@radix-ui/react-dialog";
import type { ReactNode } from "react";
import { IconButton } from "./IconButton";
import { useTranslation } from "react-i18next";
import "@/components/servers/AddServerModal.css";
import "./Dialog.css";

interface DialogProps {
  open: boolean;
  /** Called for every way out: the close button, Escape, a click on the backdrop. */
  onClose: () => void;
  title: ReactNode;
  /** Matches the `.modal-panel-*` widths the hand-built modals already use. */
  size?: "sm" | "md" | "lg";
  /** Controls sitting left of the close button, e.g. a refresh or help button. */
  headerActions?: ReactNode;
  /** Rendered between the header and the body - the tab strip, in practice. */
  belowHeader?: ReactNode;
  /**
   * Off while something destructive is in flight: a stray Escape or a click
   * on the backdrop should not abandon a delete that is already running.
   */
  dismissable?: boolean;
  children: ReactNode;
}

const SIZE_CLASS = { sm: "modal-panel-sm", md: "", lg: "modal-panel-lg" } as const;

/**
 * The app's modal.
 *
 * **What this replaces.** `useModalDialog` already did the hard part -
 * focus trap, Escape, focus restore, `role="dialog"`, and the mousedown
 * subtlety that stops a text selection dragged past the panel from closing
 * the dialog. This keeps every one of those behaviours; it exists for the
 * three it did not cover.
 *
 * 1. **Scroll lock.** The page behind a modal used to scroll under the
 *    wheel, which reads as the dialog itself failing to scroll.
 * 2. **A portal.** The panel now renders at the end of `body` rather than
 *    wherever in the tree it was written. `position: fixed` stops meaning
 *    "relative to the window" as soon as any ancestor has a `transform`,
 *    `filter` or `will-change` - so a dialog opened from inside an animated
 *    card was one stylesheet change away from being clipped by it.
 * 3. **The background goes `aria-hidden`.** Sighted users get the dimmed
 *    backdrop; without this a screen reader still walks the page underneath
 *    as though the dialog were just another section.
 *
 * The markup keeps the existing `.modal-*` class names, so the dialogs look
 * exactly as they did and their bodies move across unedited.
 */
export function Dialog({ open, onClose, title, size = "md", headerActions, belowHeader, dismissable = true, children }: DialogProps) {
  const { t } = useTranslation();

  return (
    <RadixDialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="modal-backdrop dialog-overlay" />
        <RadixDialog.Content
          className={`modal-panel dialog-panel ${SIZE_CLASS[size]}`.trim()}
          onEscapeKeyDown={(event) => !dismissable && event.preventDefault()}
          onPointerDownOutside={(event) => !dismissable && event.preventDefault()}
          onInteractOutside={(event) => !dismissable && event.preventDefault()}
        >
          <div className="modal-header">
            <RadixDialog.Title className="modal-title">{title}</RadixDialog.Title>
            <div className="modal-header-actions">
              {headerActions}
              <RadixDialog.Close asChild>
                <IconButton icon="x" size="sm" title={t("common.close")} />
              </RadixDialog.Close>
            </div>
          </div>
          {belowHeader}
          {children}
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}
