import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

/**
 * Files dragged from the desktop onto the window.
 *
 * **Why this is not `ondrop` on a div.** Tauri intercepts file drops before
 * the page sees them, so the DOM events never fire - which is why dragging a
 * file onto this app used to do nothing at all. The webview reports them
 * instead, and reports something better than the browser would: the file's
 * **path on disk**, not an opaque `File` handle. Every upload here already
 * takes a local path and streams it over SFTP from Rust, so a dropped file
 * goes down exactly the same road as one chosen through the picker, progress
 * bar and retry included.
 *
 * The subscription is window-wide - there is one webview, and it cannot be
 * scoped to an element - so only one screen should use this at a time, and
 * `enabled` exists to say when. Returns whether a drag is currently over the
 * window, for the drop target to show itself.
 */
export function useFileDrop(onDrop: (paths: string[]) => void, enabled = true) {
  const [dragging, setDragging] = useState(false);
  // Read through a ref so a handler that closes over the current directory
  // does not have to re-subscribe every time somebody navigates.
  const latest = useRef(onDrop);
  latest.current = onDrop;

  useEffect(() => {
    if (!enabled) {
      setDragging(false);
      return;
    }
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    // `getCurrentWebview()` reads Tauri's injected metadata synchronously and
    // throws when it is absent - the Vite dev page opened in a plain browser,
    // where `__TAURI_INTERNALS__` has no window metadata. That throw is not a
    // rejected promise, so the `.catch` below never saw it and the whole page
    // (Files, application Files tab) crashed instead of simply going without
    // drag-and-drop. Guard the synchronous call too.
    let webview: ReturnType<typeof getCurrentWebview>;
    try {
      webview = getCurrentWebview();
    } catch {
      return;
    }

    void webview
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          setDragging(true);
        } else if (payload.type === "leave") {
          setDragging(false);
        } else if (payload.type === "drop") {
          setDragging(false);
          if (payload.paths.length > 0) latest.current(payload.paths);
        }
      })
      .then((fn) => {
        // The screen can unmount while this promise is still in flight; without
        // the flag the listener would outlive it and upload into a directory
        // nobody is looking at any more.
        if (cancelled) void fn();
        else unlisten = fn;
      })
      // Outside Tauri - the Vite dev page opened in a plain browser - there is
      // no webview to subscribe to. Dropping files is simply unavailable
      // there, which is not worth an unhandled rejection in the console.
      .catch(() => undefined);

    return () => {
      cancelled = true;
      setDragging(false);
      void unlisten?.();
    };
  }, [enabled]);

  return dragging;
}
