import Lenis from "lenis";
// The package's own stylesheet. Only two of its rules reach a wrapper-mounted
// instance - `overscroll-behavior: contain` on the opted-out scrollers, and
// killing pointer events on iframes mid-scroll - but they are its rules to
// own rather than ours to copy.
import "lenis/dist/lenis.css";
import { useEffect, type RefObject } from "react";

/**
 * Elements inside the content area that scroll on their own.
 *
 * Lenis captures wheel events on the wrapper, so anything scrollable *inside*
 * it would otherwise scroll the page instead of itself. Portalled surfaces -
 * modals, dropdowns, tooltips - are absent from this list because they render
 * at the end of `body`, outside the wrapper, and never reach these handlers.
 *
 * Anything added later can opt out with `data-lenis-prevent`, which Lenis
 * honours on its own; this list covers the scrollers that predate it.
 */
export const OWN_SCROLLERS = [
  ".application-console-output",
  ".container-logs-output",
  ".vibe-ai-transcript",
  ".command-console-transcript",
  ".vibe-ai-preview-body",
  ".xterm-viewport",
  ".xterm-screen",
  ".cm-scroller",
  "[data-lenis-prevent]",
].join(",");

/**
 * Interpolated wheel scrolling for the main content area.
 *
 * **The measurement this exists for.** Scrolling this app by hand for five
 * seconds produced 721 frames, every one of them cheap - a 7ms median, not a
 * single frame over 20ms - and the scroll position changed in only 203 of
 * them. Each wheel notch arrives as a flat 100px and the browser's own
 * animation spends it in about four frames, leaving ten frames of stillness
 * before the next notch. The picture is not slow; it is intermittent, and on
 * a 144Hz display intermittent reads as slow.
 *
 * So this smooths *when* the distance is spent, not how fast anything is
 * drawn. It cannot help a page whose frames are expensive, and would make one
 * worse by handing scrolling to the same busy thread.
 *
 * **Why `duration` and not `lerp`.** A lerp is applied per frame, so its
 * meaning changes with the refresh rate: a value tuned at 60Hz converges two
 * and a half times faster at 144Hz and gives most of the stutter back. A
 * duration is wall-clock and behaves the same on any display. Lenis supplies
 * its own front-loaded easing alongside it, which is what keeps a short
 * duration from feeling like a delay - most of the distance is covered in the
 * first third of it.
 *
 * Attached to `.app-layout-content` because that is the element that actually
 * scrolls here; the window itself never does.
 *
 * `prefers-reduced-motion` is honoured by Lenis and deliberately not
 * overridden - interpolated scrolling is exactly the kind of movement people
 * turn that setting on to avoid.
 */
export function useSmoothScroll(wrapperRef: RefObject<HTMLElement | null>, contentRef: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const wrapper = wrapperRef.current;
    const content = contentRef.current;
    if (!wrapper || !content) return;

    const lenis = new Lenis({
      wrapper,
      content,
      autoRaf: true,
      smoothWheel: true,
      // Touch is left alone: a trackpad and a touch screen interpolate in the
      // driver already, and smoothing on top of that is what makes a
      // two-finger scroll feel like it is sliding on ice.
      syncTouch: false,
      // Long enough to bridge the ~100ms between wheel notches, so the
      // position keeps moving instead of arriving and waiting; short enough
      // that the content is not still drifting after the wheel stops.
      duration: 0.35,
      prevent: (node) => node instanceof HTMLElement && node.closest(OWN_SCROLLERS) !== null,
    });

    /*
     * Tells Lenis the page changed length.
     *
     * It re-measures only when one of its own two ResizeObservers fires: one
     * on the wrapper, one on the content element. The wrapper's only changes
     * with the window. The content element's never fires at all, because
     * `.app-layout-scroll-content` is `height: 100%` - which is what gives
     * the terminal a definite height to fill - and its children overflow it
     * instead of stretching it. Measured in the running app: appending a
     * 2000px child left the content box at 576px while the wrapper's
     * scrollHeight went to 2420.
     *
     * So the scroll range stayed frozen at whatever the first page needed,
     * and a longer one could not be scrolled to the bottom until the window
     * was resized. This is the cost of that `height: 100%`, paid here rather
     * than by giving the height back and breaking the terminal again.
     */
    let frame = 0;
    let measured = wrapper.scrollHeight;

    const remeasure = () => {
      frame = 0;
      // The read is the expensive part, so nothing else happens unless it
      // actually changed.
      if (wrapper.scrollHeight === measured) return;
      measured = wrapper.scrollHeight;
      lenis.resize();
    };

    const growth = new MutationObserver((records) => {
      // The terminal and the console rewrite their own DOM continuously and
      // scroll inside themselves, so they never change how long the page is.
      // Ignoring them keeps this from running on every line of output.
      if (records.every((record) => record.target instanceof Element && record.target.closest(OWN_SCROLLERS))) return;
      if (!frame) frame = requestAnimationFrame(remeasure);
    });
    growth.observe(content, { childList: true, subtree: true });

    /*
     * The other way a page gets longer: an image arrives.
     *
     * The observer above fires when the markup changes, which is before any
     * `<img>` in it has loaded - and an image with no dimensions is a 2px
     * box until it does. Measured on the Guide's Panel page: 2px before the
     * screenshot loaded, 440px after, and nothing in between that the
     * observer can see, because finishing a download is not a DOM mutation
     * and the content element's own height never changes (see above).
     *
     * So the scroll range was measured against a page 438px shorter than the
     * one on screen, and the reader could not reach the end of it. On a page
     * that is mostly one screenshot, the range left over is small enough
     * that it reads as not scrolling at all.
     *
     * `load` does not bubble, so this listens in the capture phase - the only
     * way one listener on the container hears about every image inside it.
     * `error` counts too: a broken image collapses to its alt text, which
     * changes the length just as much.
     */
    const mediaSettled = () => {
      if (!frame) frame = requestAnimationFrame(remeasure);
    };
    content.addEventListener("load", mediaSettled, true);
    content.addEventListener("error", mediaSettled, true);

    return () => {
      if (frame) cancelAnimationFrame(frame);
      growth.disconnect();
      content.removeEventListener("load", mediaSettled, true);
      content.removeEventListener("error", mediaSettled, true);
      lenis.destroy();
    };
  }, [wrapperRef, contentRef]);
}
