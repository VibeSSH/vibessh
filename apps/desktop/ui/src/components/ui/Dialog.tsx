import * as RadixDialog from "@radix-ui/react-dialog";
import { motion, useReducedMotion } from "motion/react";
import type { ReactNode } from "react";
import { IconButton } from "./IconButton";
import { useTranslation } from "react-i18next";
import "@/components/servers/AddServerModal.css";
import "./Dialog.css";

interface DialogProps {
  open: boolean;
  /** Called for every way out: the close button and Escape. A click on the backdrop is not one. */
  onClose: () => void;
  title: ReactNode;
  /** Matches the `.modal-panel-*` widths the hand-built modals already use. */
  size?: "sm" | "md" | "lg";
  /** Controls sitting left of the close button, e.g. a refresh or help button. */
  headerActions?: ReactNode;
  /** Rendered between the header and the body - the tab strip, in practice. */
  belowHeader?: ReactNode;
  /**
   * Off while something destructive is in flight: a stray Escape should not
   * abandon a delete that is already running. (A click on the backdrop never
   * closes a dialog at all.)
   */
  dismissable?: boolean;
  children: ReactNode;
}

const SIZE_CLASS = { sm: "modal-panel-sm", md: "", lg: "modal-panel-lg" } as const;

/**
 * How long a dialog takes to arrive.
 *
 * Short on purpose. Long enough to read as movement rather than a jump,
 * short enough that somebody opening a dialog to type into it is not made to
 * wait for the animation - which is the failure mode of every modal that
 * animates for a third of a second.
 */
const ENTER_SECONDS = 0.16;

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
  /**
   * Nothing moves for somebody who asked the operating system for that.
   *
   * A modal that scales into place is a nicety; a modal that scales into
   * place for a person who set "reduce motion" because movement makes them
   * ill is a defect. `0` keeps the same code path and simply arrives at the
   * end of it immediately.
   */
  const still = useReducedMotion();
  const duration = still ? 0 : ENTER_SECONDS;

  return (
    <RadixDialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <RadixDialog.Portal>
        {/* `asChild` hands Radix's behaviour to a motion element rather than
            wrapping one around it: an extra div here would sit between the
            backdrop and the panel and take the clicks meant for either.

            Entry only, no exit. An exit animation needs the dialog to stay
            mounted after it closes (`forceMount` plus `AnimatePresence`),
            which changes when every caller's state unmounts - a lot of new
            behaviour for the half of the movement nobody watches. */}
        <RadixDialog.Overlay asChild>
          <motion.div
            className="modal-backdrop dialog-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration }}
          />
        </RadixDialog.Overlay>
        <RadixDialog.Content
          asChild
          onEscapeKeyDown={(event) => !dismissable && event.preventDefault()}
          // Never closed by a click outside, whatever `dismissable` says - the
          // same rule as `useModalDialog`: that click was usually meant for
          // something behind the dialog, and it cost whatever was on it.
          onPointerDownOutside={(event) => event.preventDefault()}
          onInteractOutside={(event) => event.preventDefault()}
        >
          <motion.div
            className={`modal-panel dialog-panel ${SIZE_CLASS[size]}`.trim()}
            // Barely a scale. A dialog that grows from 0.9 reads as a
            // notification popping up; this reads as the same panel settling.
            initial={{ opacity: 0, scale: 0.985 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={{ duration, ease: "easeOut" }}
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
          </motion.div>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}
