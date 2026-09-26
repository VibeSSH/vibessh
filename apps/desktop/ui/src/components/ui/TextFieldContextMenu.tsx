import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import type { MouseEvent as ReactMouseEvent } from "react";
import { useContextMenu } from "@/components/ui/ContextMenu";
import { copyToClipboard } from "@/utils/copyToClipboard";
import { toastError } from "@/stores/toastStore";

/** Input types a person types text into - the ones a right-click should
 * offer cut, copy and paste in. Checkboxes, buttons and the like have no
 * text to act on. */
const TEXT_INPUT_TYPES = new Set(["text", "password", "email", "search", "url", "tel", "number", ""]);

type TextField = HTMLInputElement | HTMLTextAreaElement;

function textFieldFrom(target: EventTarget | null): TextField | null {
  if (target instanceof HTMLTextAreaElement) return target;
  if (target instanceof HTMLInputElement && TEXT_INPUT_TYPES.has(target.type)) return target;
  return null;
}

/** The field's selection, or null where the input type has none (`number`). */
function selectionOf(field: TextField): { start: number; end: number } | null {
  try {
    if (field.selectionStart === null || field.selectionEnd === null) return null;
    return { start: field.selectionStart, end: field.selectionEnd };
  } catch {
    return null;
  }
}

/**
 * Puts `text` in place of the field's selection the way typing would.
 *
 * `insertText` first: it goes through the browser's own editing, so the
 * change lands in the field's undo history and fires the `input` event a
 * React-controlled field listens for. The fallback writes the value
 * directly and fires that event itself, for an engine that refuses.
 */
function replaceSelection(field: TextField, text: string) {
  field.focus();
  if (document.execCommand("insertText", false, text)) return;
  const selection = selectionOf(field) ?? { start: field.value.length, end: field.value.length };
  field.setRangeText(text, selection.start, selection.end, "end");
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

/**
 * Cut, copy, paste and select-all on a right-click in any text field.
 *
 * **Why this has to exist.** `main.tsx` suppresses the webview's own
 * context menu, which otherwise offered Back, Reload and Inspect over the
 * whole app - and took Paste out of every field with it, so pasting a code
 * from an email into the sign-in dialog was a keyboard shortcut or nothing.
 * Mounted once for the whole app rather than per field, since every text
 * field wants the same four things.
 *
 * A component that already opens its own menu - the file editors, a file
 * row - calls `preventDefault` first, and this steps aside for it. A
 * password field does not copy or cut its contents, as in any browser.
 */
export function TextFieldContextMenu() {
  const { t } = useTranslation();
  const menu = useContextMenu();

  useEffect(() => {
    function handle(event: MouseEvent) {
      // Shift is the way through to the webview's own menu, as everywhere
      // else in the app (`main.tsx`).
      if (event.defaultPrevented || event.shiftKey) return;
      const field = textFieldFrom(event.target);
      if (!field) return;

      const selection = selectionOf(field);
      const selected = selection ? field.value.slice(selection.start, selection.end) : "";
      const editable = !field.readOnly && !field.disabled;
      const secret = field instanceof HTMLInputElement && field.type === "password";
      const copyLabels = { copied: t("common.copied"), failed: t("common.copyFailed") };

      menu.open(event as unknown as ReactMouseEvent, [
        {
          label: t("editorMenu.cut"),
          icon: "scissors",
          disabled: selected === "" || !editable || secret,
          onClick: () => {
            void copyToClipboard(selected, copyLabels).then((copied) => {
              // Only once the text is on the clipboard - a cut that deleted
              // first and then failed to copy would lose it.
              if (copied) replaceSelection(field, "");
            });
          },
        },
        {
          label: t("editorMenu.copy"),
          icon: "copy",
          disabled: selected === "" || secret,
          onClick: () => void copyToClipboard(selected, copyLabels),
        },
        {
          label: t("editorMenu.paste"),
          icon: "clipboard",
          disabled: !editable,
          onClick: () => {
            navigator.clipboard
              .readText()
              .then((text) => {
                if (text !== "") replaceSelection(field, text);
              })
              // A refused read is worth saying: pasting nothing looks exactly
              // like pasting an empty clipboard.
              .catch(() => toastError(t("editorMenu.pasteFailed")));
          },
        },
        {
          label: t("editorMenu.selectAll"),
          icon: "square",
          disabled: field.value === "",
          onClick: () => {
            field.focus();
            field.select();
          },
        },
      ]);
    }
    document.addEventListener("contextmenu", handle);
    return () => document.removeEventListener("contextmenu", handle);
  }, [menu, t]);

  return menu.element;
}
