import { useTranslation } from "react-i18next";
import type { MouseEvent as ReactMouseEvent, RefObject } from "react";
import type { EditorView } from "@codemirror/view";
import { useContextMenu } from "@/components/ui/ContextMenu";
import { copyToClipboard } from "@/utils/copyToClipboard";
import { toastError } from "@/stores/toastStore";

/**
 * Copy, cut, paste and select-all on a right-click inside a file editor.
 *
 * **Why this has to exist at all.** The app suppresses the webview's own
 * context menu (see `main.tsx`) so components can offer their own, and the
 * file editors never did - so right-clicking a selection in a config file did
 * nothing whatsoever. Every other way of copying out of an editor is a
 * keyboard shortcut somebody has to already know.
 *
 * Shared by both editors rather than written twice: the Application Files
 * editor and the Node Files editor are separate components for reasons that
 * have nothing to do with what a right-click should do.
 *
 * The commands are dispatched against CodeMirror's own state rather than
 * `document.execCommand`, which is deprecated and, in a webview, silently
 * inconsistent about what it will do to a selection it did not make.
 */
export function useEditorContextMenu(viewRef: RefObject<EditorView | null>, options: { editable: boolean }) {
  const { t } = useTranslation();
  const menu = useContextMenu();

  function selectionText(view: EditorView): string {
    const { from, to } = view.state.selection.main;
    return view.state.sliceDoc(from, to);
  }

  async function paste(view: EditorView) {
    let text: string;
    try {
      text = await navigator.clipboard.readText();
    } catch {
      // A refused clipboard read is the one failure worth saying out loud:
      // pasting nothing looks exactly like pasting an empty clipboard.
      toastError(t("editorMenu.pasteFailed"));
      return;
    }
    if (text === "") return;
    const { from, to } = view.state.selection.main;
    view.dispatch({
      changes: { from, to, insert: text },
      selection: { anchor: from + text.length },
      // Keeps the caret in view when the paste lands off-screen, which it
      // does whenever the selection was made by scrolling rather than typing.
      scrollIntoView: true,
    });
    view.focus();
  }

  function open(event: ReactMouseEvent) {
    const view = viewRef.current;
    if (!view) return;
    const selected = selectionText(view);
    const copyLabels = { copied: t("common.copied"), failed: t("common.copyFailed") };

    menu.open(event, [
      {
        label: t("editorMenu.copy"),
        icon: "copy",
        disabled: selected === "",
        onClick: () => void copyToClipboard(selected, copyLabels),
      },
      {
        label: t("editorMenu.cut"),
        icon: "scissors",
        // Nothing to cut, or nowhere to cut from: a read-only editor offers
        // the row greyed rather than hiding it, so the menu keeps the same
        // shape wherever it is opened.
        disabled: selected === "" || !options.editable,
        onClick: () => {
          void copyToClipboard(selected, copyLabels).then((copied) => {
            // Only after the text is safely on the clipboard - a cut that
            // deleted first and then failed to copy would lose it outright.
            if (!copied) return;
            const { from, to } = view.state.selection.main;
            view.dispatch({ changes: { from, to, insert: "" }, scrollIntoView: true });
            view.focus();
          });
        },
      },
      {
        label: t("editorMenu.paste"),
        icon: "clipboard",
        disabled: !options.editable,
        onClick: () => void paste(view),
      },
      {
        label: t("editorMenu.selectAll"),
        icon: "square",
        // Dispatched rather than imported from `@codemirror/commands`, which
        // is not a declared dependency of this project - it only resolves
        // today because something else pulls it in.
        onClick: () => {
          view.dispatch({ selection: { anchor: 0, head: view.state.doc.length } });
          view.focus();
        },
      },
    ]);
  }

  return { onContextMenu: open, element: menu.element };
}
