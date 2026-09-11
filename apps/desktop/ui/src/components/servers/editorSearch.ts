import type { Extension } from "@codemirror/state";
import { EditorState } from "@codemirror/state";
import { highlightSelectionMatches, search } from "@codemirror/search";

/**
 * Find-in-file for the editors, in the user's own language.
 *
 * `basicSetup` already binds Ctrl+F, so the search itself was reachable
 * before this - by anybody who guessed. A config file is where somebody
 * hunts for one key among two thousand lines, which makes a shortcut with
 * no visible affordance the wrong way to offer it; the panel is opened from
 * a button in the editor's header as well.
 *
 * The panel is CodeMirror's own, and every string in it goes through
 * `EditorState.phrases`. Without that it is an English box in a Polish
 * interface - the one piece of the editor a user reads words from rather
 * than just looking at.
 */
export function searchExtensions(phrases: Record<string, string>): Extension[] {
  return [
    // Above the document rather than below it. The editor's bottom edge is
    // where the horizontal scrollbar and the window edge already are, and a
    // panel there covers the last lines of the file while you search it.
    search({ top: true }),
    // Every other occurrence of the current selection, so finding one match
    // shows where the rest are without stepping through them.
    highlightSelectionMatches(),
    EditorState.phrases.of(phrases),
  ];
}

/**
 * The panel's own strings, in the interface's language.
 *
 * Built here rather than at each call site so the two editors cannot drift
 * apart, and so a key CodeMirror looks up but nobody translated is a gap in
 * one file. An untranslated key falls back to its English self: a
 * degradation rather than a break - the control still works, it just speaks
 * the wrong language.
 *
 * `Replace` and `replace` are both here and are not duplicates. CodeMirror
 * uses the capitalised one for the input's placeholder and the lower-case
 * one for the button, and Polish needs different words for the two anyway:
 * one names a field, the other commands an action.
 */
export function searchPhrases(t: (key: string) => string): Record<string, string> {
  return {
    Find: t("editorSearch.find"),
    Replace: t("editorSearch.replaceField"),
    next: t("editorSearch.next"),
    previous: t("editorSearch.previous"),
    all: t("editorSearch.all"),
    "match case": t("editorSearch.matchCase"),
    regexp: t("editorSearch.regexp"),
    "by word": t("editorSearch.byWord"),
    replace: t("editorSearch.replaceAction"),
    "replace all": t("editorSearch.replaceAll"),
    close: t("editorSearch.close"),
  };
}
