/**
 * The commands sent to an application's console, and walking back through
 * them with the arrow keys.
 *
 * Every shell has had this for forty years, and the reason is the same here:
 * console commands are long, typed by hand, and almost never sent once. `lp
 * user CrispiDEV parent add mod` is retyped for the next player, a server
 * command is repeated after a restart to see whether it took.
 *
 * Kept in `localStorage`, per application. Which commands somebody has been
 * sending is a fact about this desktop's session rather than about the
 * infrastructure - the same reasoning that keeps the open tab strip there -
 * and it has to survive the console card unmounting, which it does every time
 * somebody looks at another tab.
 */

const STORED_PREFIX = "vibessh.consoleHistory.";

/**
 * How many commands to keep per application.
 *
 * Long enough to reach yesterday's command, short enough that the store stays
 * small when somebody scripts a server through this box for an hour.
 */
export const MAX_HISTORY = 100;

function storageKey(applicationId: string): string {
  return `${STORED_PREFIX}${applicationId}`;
}

/** Oldest first, which is the order the arrow keys walk backwards through. */
export function loadHistory(applicationId: string): string[] {
  try {
    const raw = window.localStorage.getItem(storageKey(applicationId));
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    // Read defensively: this is storage an older version of the app wrote.
    return parsed.filter((entry): entry is string => typeof entry === "string").slice(-MAX_HISTORY);
  } catch {
    // Private browsing, cleared storage, or a value this version cannot
    // read. No history is a fine outcome.
    return [];
  }
}

/**
 * Records a command, and answers with the history as it now stands.
 *
 * A command identical to the one before it is not recorded twice - that is
 * `bash`'s `ignoredups`, and it exists because the single most repeated
 * command is the one somebody just sent and is about to send again. Two
 * copies in a row would mean pressing Up twice to get past it.
 */
export function rememberCommand(applicationId: string, command: string): string[] {
  const trimmed = command.trim();
  if (!trimmed) return loadHistory(applicationId);

  const history = loadHistory(applicationId);
  if (history[history.length - 1] === trimmed) return history;

  const next = [...history, trimmed].slice(-MAX_HISTORY);
  try {
    window.localStorage.setItem(storageKey(applicationId), JSON.stringify(next));
  } catch {
    // Storage unavailable or full. The history still works for this session.
  }
  return next;
}

/**
 * Where the arrow keys have walked to.
 *
 * `index` is a position in the history array, or `null` for "not walking -
 * what is in the box is what somebody typed". `draft` is that typed text, put
 * aside on the first press so that walking all the way back down returns it
 * rather than an empty box.
 */
export interface HistoryPosition {
  index: number | null;
  draft: string;
}

export const NOT_BROWSING: HistoryPosition = { index: null, draft: "" };

/**
 * One press of Up or Down.
 *
 * Answers `null` when the press means nothing - no history at all, or Down
 * while not walking - so the caller can leave the keystroke to the input,
 * where Down does the ordinary thing.
 *
 * The conventions are the shell's, because that is what fingers expect:
 * Up from the newest entry stays on it rather than wrapping round to the
 * oldest, and Down past the newest hands back the draft.
 */
export function stepThroughHistory(
  history: string[],
  position: HistoryPosition,
  typed: string,
  direction: "older" | "newer",
): { value: string; position: HistoryPosition } | null {
  if (history.length === 0) return null;

  if (direction === "older") {
    if (position.index === null) {
      const index = history.length - 1;
      return { value: history[index], position: { index, draft: typed } };
    }
    const index = Math.max(0, position.index - 1);
    return { value: history[index], position: { ...position, index } };
  }

  if (position.index === null) return null;

  if (position.index >= history.length - 1) {
    return { value: position.draft, position: NOT_BROWSING };
  }

  const index = position.index + 1;
  return { value: history[index], position: { ...position, index } };
}
