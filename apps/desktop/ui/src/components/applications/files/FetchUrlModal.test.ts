import { describe, expect, it } from "vitest";
import { fileNameFromUrl } from "./FetchUrlModal";

describe("fileNameFromUrl", () => {
  it("takes the last path segment, decoded", () => {
    expect(fileNameFromUrl("https://cdn.modrinth.com/data/abc/versions/1.0/LuckPerms-Bukkit-5.4.jar")).toBe("LuckPerms-Bukkit-5.4.jar");
    expect(fileNameFromUrl("https://example.com/files/My%20World.zip?download=1")).toBe("My World.zip");
  });

  it("gives nothing for a link without a file in it", () => {
    expect(fileNameFromUrl("https://example.com/")).toBe("");
    expect(fileNameFromUrl("not a link")).toBe("");
  });

  it("never suggests a name that steps out of the folder", () => {
    expect(fileNameFromUrl("https://example.com/%2E%2E%2Fetc%2Fpasswd")).toBe("etcpasswd");
    expect(fileNameFromUrl("https://example.com/..%5Cx.jar")).toBe("x.jar");
  });
});
