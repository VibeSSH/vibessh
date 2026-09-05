import { describe, expect, it } from "vitest";
import { fileIcon } from "./fileIcons";

describe("telling a folder from a file at a glance", () => {
  it("gives a directory its own icon and tone whatever it is called", () => {
    // The case this exists for: in a plugins directory these two sit next to
    // each other, and they used to differ only by the shape of a small grey
    // outline.
    const folder = fileIcon("landmc-auth", true);
    const jar = fileIcon("landmc-auth.jar", false);

    expect(folder.tone).toBe("folder");
    expect(jar.tone).toBe("archive");
    expect(folder.name).not.toBe(jar.name);
  });

  it("reads the extension after the last dot, not the first", () => {
    // `paper-1.21.11.jar` is a jar. Splitting on the first dot would make it
    // a "21", which matches nothing and would render as a plain file.
    expect(fileIcon("paper-1.21.11.jar", false).tone).toBe("archive");
  });

  it("treats a dotfile as having no extension", () => {
    // `.gitignore` is not a file of type "gitignore".
    expect(fileIcon(".gitignore", false).tone).toBe("plain");
  });

  it("does not care about case", () => {
    expect(fileIcon("SERVER.JAR", false).tone).toBe("archive");
  });

  it("falls back to plain rather than guessing", () => {
    expect(fileIcon("banned-ips.wat", false).tone).toBe("plain");
    expect(fileIcon("README", false).tone).toBe("plain");
  });

  it.each([
    ["server.properties", "config"],
    ["bukkit.yml", "config"],
    ["ops.json", "config"],
    ["level.dat", "data"],
    ["start.sh", "script"],
    ["latest.log", "log"],
    ["server.key", "secret"],
  ])("%s reads as %s", (name, tone) => {
    expect(fileIcon(name, false).tone).toBe(tone);
  });
});
