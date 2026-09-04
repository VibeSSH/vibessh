import { describe, expect, it } from "vitest";
import { applyTheme, clearTheme, contrastRatio, deriveContrast, parseColor, parseTheme } from "./applyTheme";
import { BUILT_IN_THEMES } from "./themes";
import { THEMABLE_TOKENS, type ThemableToken } from "./tokens";

const NORD = BUILT_IN_THEMES.find((theme) => theme.id === "nord")!;

describe("what a theme is allowed to contain", () => {
  it.each(["var(--accent)", "color-mix(in srgb, red 50%, blue)", "url(http://example.com/x.png)", "red", "rgb(0 0 0)", "#12345", ""])(
    "rejects %s",
    (value) => {
      // Hex only. Every one of these is valid CSS, and each would let a theme
      // file reach past the closed list of names - into a token it may not
      // set, or out to the network.
      expect(parseColor(value)).toBeNull();
    },
  );

  it.each(["#abc", "#AABBCC", "#aabbccdd"])("accepts %s", (value) => {
    expect(parseColor(value)).toBe(value.toLowerCase());
  });

  it("refuses a theme that is missing a colour rather than applying half of one", () => {
    const incomplete = { id: "x", name: "X", colors: { ...NORD.colors } } as { colors: Record<string, string> };
    delete incomplete.colors["--accent"];

    expect(parseTheme(incomplete)).toBeNull();
  });

  it("drops keys it does not know instead of passing them through", () => {
    const parsed = parseTheme({ id: "x", name: "X", colors: { ...NORD.colors, "--z-modal": "#ffffff", "--space-4": "#ffffff" } });

    expect(parsed).not.toBeNull();
    expect(Object.keys(parsed!.colors).sort()).toEqual([...THEMABLE_TOKENS].sort());
  });
});

describe("the colours the app works out for itself", () => {
  it.each(BUILT_IN_THEMES.map((theme) => [theme.name, theme] as const))("%s ends up readable", (_name, theme) => {
    const derived = deriveContrast(theme.colors);

    // The two that a person choosing colours should not have to reason about:
    // the label on the accent button, and the danger colour used as text.
    expect(contrastRatio(theme.colors["--accent"], derived["--accent-contrast"])).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(derived["--danger-text"], theme.colors["--surface-1"])).toBeGreaterThanOrEqual(4.5);
  });

  it("puts dark text on a light accent and light text on a dark one", () => {
    const light = { ...NORD.colors, "--accent": "#ffe08a" } as Record<ThemableToken, string>;
    const dark = { ...NORD.colors, "--accent": "#1a2b4c" } as Record<ThemableToken, string>;

    expect(deriveContrast(light)["--accent-contrast"]).toBe("#04141c");
    expect(deriveContrast(dark)["--accent-contrast"]).toBe("#f7fdff");
  });
});

describe("applying a theme", () => {
  it("touches only the properties a theme is allowed to set", () => {
    document.documentElement.style.setProperty("--z-modal", "100");

    applyTheme(NORD);

    const set = new Set(Array.from(document.documentElement.style).map(String));
    // `--z-modal` was put there by this test, not by the theme - everything
    // else present must be a themable or derived token.
    const unexpected = [...set].filter((name) => name !== "--z-modal" && !THEMABLE_TOKENS.includes(name as ThemableToken) && !name.startsWith("--accent-contrast") && !name.startsWith("--danger-text"));
    expect(unexpected).toEqual([]);
    expect(document.documentElement.style.getPropertyValue("--accent")).toBe(NORD.colors["--accent"]);

    clearTheme();
    expect(document.documentElement.style.getPropertyValue("--accent")).toBe("");
    // Clearing a theme must not take anything else with it.
    expect(document.documentElement.style.getPropertyValue("--z-modal")).toBe("100");
    document.documentElement.style.removeProperty("--z-modal");
  });
});
