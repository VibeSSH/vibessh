import { Channel, invoke } from "@tauri-apps/api/core";

/**
 * Starts the operating system's own drag of a local file, so it can be
 * dropped on the desktop or into an Explorer/Finder window.
 *
 * A browser drag cannot carry a file out of the window; only a native call
 * can, which is what `tauri-plugin-drag` provides. The file has to exist on
 * this computer first - the caller downloads it while the button is still
 * held (see `prepareApplicationFileDragOut`).
 */
export async function dragFileOut(localPath: string, label: string): Promise<void> {
  const onEvent = new Channel<unknown>();
  await invoke("plugin:drag|start_drag", { item: [localPath], image: dragImage(label), options: { mode: "copy" }, onEvent });
}

/**
 * What follows the cursor: the file's name on a small muted card. Drawn
 * rather than shipped as an asset because it carries the name, and the
 * plugin takes a PNG data URL.
 */
function dragImage(label: string): string {
  const scale = window.devicePixelRatio || 1;
  const width = 240;
  const height = 34;
  const canvas = document.createElement("canvas");
  canvas.width = width * scale;
  canvas.height = height * scale;
  const context = canvas.getContext("2d");
  if (!context) return canvas.toDataURL("image/png");
  context.scale(scale, scale);
  const styles = getComputedStyle(document.documentElement);
  const token = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;

  context.fillStyle = token("--surface-2", "#191b20");
  context.strokeStyle = token("--border", "#2a2d35");
  context.beginPath();
  context.roundRect(0.5, 0.5, width - 1, height - 1, 6);
  context.fill();
  context.stroke();

  context.fillStyle = token("--text-primary", "#ecedef");
  context.font = `500 13px ${token("--font-sans", "system-ui, sans-serif")}`;
  context.textBaseline = "middle";
  let text = label;
  while (context.measureText(text).width > width - 24 && text.length > 4) text = `${text.slice(0, -2)}…`;
  context.fillText(text, 12, height / 2);
  return canvas.toDataURL("image/png");
}
