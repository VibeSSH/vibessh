/**
 * Screenshots that ship with the guide.
 *
 * Bundled through Vite rather than referenced by path, because a packaged
 * desktop app has no `docs/` directory to serve from - the same reason the
 * documents themselves are embedded. A markdown file writes
 * `![caption](images/ports-form.png)` and this turns that into whatever the
 * bundler produced.
 *
 * An unknown name resolves to `undefined` and the figure is skipped, so a
 * screenshot that has not been captured yet leaves a paragraph gap rather
 * than a broken image icon in the middle of a manual.
 */
const files = import.meta.glob("../../../../../shared/guide/images/*.{png,jpg,webp}", { query: "?url", import: "default", eager: true }) as Record<
  string,
  string
>;

const byName = new Map<string, string>();
for (const [path, url] of Object.entries(files)) {
  const name = path.split("/").pop();
  if (name) byName.set(name, url);
}

export function guideImage(src: string): string | undefined {
  return byName.get(src.split("/").pop() ?? src);
}
