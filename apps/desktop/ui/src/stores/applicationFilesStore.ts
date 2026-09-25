import type { RemoteFileEntry } from "@/types/files";

/**
 * Where each Application's Files tab was left, and any edit not yet saved.
 *
 * The Application page remounts on every switch between Applications - it is
 * keyed by id so one Application's panels never reconcile into another's - so
 * everything the Files tab held in component state went with it. Opening a
 * config, glancing at another server and coming back landed on the top of
 * the tree with the file closed, and an unsaved edit gone without a word.
 *
 * Memory only, for this run of the app. A draft is somebody's unsaved work,
 * and writing it to disk would be a decision about their files they never
 * made; losing it on quit is what every editor does without a save.
 */

export interface FilesView {
  path: string;
  openFile: RemoteFileEntry | null;
}

/** An unsaved edit, with the file as it was when the edit began. */
export interface FileDraft {
  base: string;
  content: string;
}

const views = new Map<string, FilesView>();
const drafts = new Map<string, FileDraft>();

const draftKey = (applicationId: string, path: string) => `${applicationId}\u0000${path}`;

export function rememberedFilesView(applicationId: string): FilesView | undefined {
  return views.get(applicationId);
}

export function rememberFilesView(applicationId: string, view: FilesView): void {
  views.set(applicationId, view);
}

/** Edits somebody chose to throw away, so the editor closing over them does not keep them as a draft. */
const discarded = new Set<string>();

/**
 * Reads a draft without removing it. Removal is `dropFileDraft`, called once
 * the draft is back in an editor: a read that also removed would lose it to
 * StrictMode, which renders twice and throws one render away.
 */
export function fileDraft(applicationId: string, path: string): FileDraft | undefined {
  return drafts.get(draftKey(applicationId, path));
}

export function dropFileDraft(applicationId: string, path: string): void {
  drafts.delete(draftKey(applicationId, path));
}

/** Kept when an editor with unsaved changes goes away - unless those changes were just discarded on purpose. */
export function keepFileDraft(applicationId: string, path: string, draft: FileDraft): void {
  const key = draftKey(applicationId, path);
  if (discarded.delete(key)) return;
  drafts.set(key, draft);
}

/** Says the edits in this file were thrown away deliberately - call before closing the editor over them. */
export function discardFileDraft(applicationId: string, path: string): void {
  const key = draftKey(applicationId, path);
  drafts.delete(key);
  discarded.add(key);
}
