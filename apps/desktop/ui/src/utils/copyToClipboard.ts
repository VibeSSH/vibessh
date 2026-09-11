import { toastError, toastSuccess } from "@/stores/toastStore";

/**
 * Puts text on the clipboard and says so.
 *
 * The saying-so is the point. Copying is invisible: the button looks the same
 * before and after, the clipboard is somewhere else, and the only way to find
 * out whether it worked is to paste somewhere and look. Every copy button in
 * the app used to be silent, which left "did that do anything?" as the honest
 * reading of pressing one.
 *
 * The failure is announced too, rather than swallowed as it was before.
 * A refused clipboard used to look exactly like a successful copy, so the
 * next paste produced whatever had been there beforehand - which for a
 * password or a connection string is worse than being told it did not work.
 */
export async function copyToClipboard(text: string, messages: { copied: string; failed: string }): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    toastSuccess(messages.copied);
    return true;
  } catch {
    toastError(messages.failed);
    return false;
  }
}
