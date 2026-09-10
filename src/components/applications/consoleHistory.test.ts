import { beforeEach, describe, expect, it } from "vitest";
import { loadHistory, MAX_HISTORY, NOT_BROWSING, rememberCommand, stepThroughHistory } from "./consoleHistory";

beforeEach(() => window.localStorage.clear());

/**
 * Walking back through commands already sent, with the arrow keys.
 *
 * The conventions here are a shell's, because that is what fingers expect
 * from a box with a blinking cursor in it: Up stops at the oldest rather than
 * wrapping, Down past the newest hands back whatever was half-typed, and the
 * same command sent twice in a row is one entry, not two.
 */
describe("stepThroughHistory", () => {
  const history = ["lp user A parent add mod", "stop", "lp user B parent add mod"];

  it("does nothing at all when nothing has been sent yet", () => {
    expect(stepThroughHistory([], NOT_BROWSING, "half typed", "older")).toBeNull();
    expect(stepThroughHistory([], NOT_BROWSING, "half typed", "newer")).toBeNull();
  });

  it("gives the newest command on the first press of Up", () => {
    const stepped = stepThroughHistory(history, NOT_BROWSING, "", "older");

    expect(stepped?.value).toBe("lp user B parent add mod");
  });

  it("walks back one command at a time", () => {
    const first = stepThroughHistory(history, NOT_BROWSING, "", "older")!;
    const second = stepThroughHistory(history, first.position, first.value, "older")!;

    expect(second.value).toBe("stop");
  });

  /** Wrapping round to the newest would look like the box had cleared
   *  itself. Every shell stops here, so this one does too. */
  it("stays on the oldest rather than wrapping round", () => {
    let position = NOT_BROWSING;
    let value = "";
    for (let press = 0; press < 10; press += 1) {
      const stepped = stepThroughHistory(history, position, value, "older")!;
      position = stepped.position;
      value = stepped.value;
    }

    expect(value).toBe("lp user A parent add mod");
  });

  /** The half-typed line is the thing somebody was actually writing. Losing
   *  it to a curious press of Up is the reason shells put it aside. */
  it("hands back what was half-typed when walking past the newest again", () => {
    const up = stepThroughHistory(history, NOT_BROWSING, "lp user C par", "older")!;
    const down = stepThroughHistory(history, up.position, up.value, "newer")!;

    expect(down.value).toBe("lp user C par");
    expect(down.position).toEqual(NOT_BROWSING);
  });

  it("leaves Down alone while nobody is walking", () => {
    expect(stepThroughHistory(history, NOT_BROWSING, "typing", "newer")).toBeNull();
  });
});

describe("rememberCommand", () => {
  it("keeps commands oldest first, which is the order Up walks backwards through", () => {
    rememberCommand("app-1", "first");
    rememberCommand("app-1", "second");

    expect(loadHistory("app-1")).toEqual(["first", "second"]);
  });

  /** bash's `ignoredups`: the most repeated command is the one just sent, and
   *  two copies in a row would mean pressing Up twice to get past it. */
  it("does not record the same command twice in a row", () => {
    rememberCommand("app-1", "stop");
    rememberCommand("app-1", "stop");

    expect(loadHistory("app-1")).toEqual(["stop"]);
  });

  it("records it again when something else came between", () => {
    rememberCommand("app-1", "stop");
    rememberCommand("app-1", "start");
    rememberCommand("app-1", "stop");

    expect(loadHistory("app-1")).toEqual(["stop", "start", "stop"]);
  });

  it("ignores an empty command, and trims the one it keeps", () => {
    rememberCommand("app-1", "   ");
    rememberCommand("app-1", "  stop  ");

    expect(loadHistory("app-1")).toEqual(["stop"]);
  });

  /** One application's history is not another's - they share a storage area
   *  and differ only by the id in the key. */
  it("keeps each application's commands to itself", () => {
    rememberCommand("app-1", "lp user A parent add mod");
    rememberCommand("app-2", "stop");

    expect(loadHistory("app-1")).toEqual(["lp user A parent add mod"]);
    expect(loadHistory("app-2")).toEqual(["stop"]);
  });

  it("drops the oldest once it is full, rather than growing forever", () => {
    for (let i = 0; i < MAX_HISTORY + 5; i += 1) rememberCommand("app-1", `command ${i}`);

    const history = loadHistory("app-1");
    expect(history).toHaveLength(MAX_HISTORY);
    expect(history[0]).toBe("command 5");
    expect(history[history.length - 1]).toBe(`command ${MAX_HISTORY + 4}`);
  });

  it("survives storage written by a version that put something else there", () => {
    window.localStorage.setItem("vibessh.consoleHistory.app-1", JSON.stringify(["stop", 7, null, "start"]));

    expect(loadHistory("app-1")).toEqual(["stop", "start"]);
  });

  it("reads nothing rather than throwing when the stored value is not JSON", () => {
    window.localStorage.setItem("vibessh.consoleHistory.app-1", "{not json");

    expect(loadHistory("app-1")).toEqual([]);
  });
});
