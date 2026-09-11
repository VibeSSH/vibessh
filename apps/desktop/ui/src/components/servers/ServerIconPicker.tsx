import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { setServerIcon } from "@/services/serverService";
import { errorMessage } from "@/services/tauri";
import type { ManagedServer } from "@/stores/serversStore";
import "./ServerIconPicker.css";

/** The stored icon's edge, in pixels. Small on purpose: it is rendered at
 * 16-32px in the rail and on the card, and every pixel above that is bytes in
 * a row that `list_servers` returns for every node on every load. */
const ICON_SIZE = 64;

/** What the file input will offer. SVG is absent deliberately - see
 * `rasterize`. */
const ACCEPTED = "image/png,image/jpeg,image/webp,image/gif,image/bmp";

/** The largest file worth decoding for a 64px icon.
 *
 * Checked before anything touches the bytes. Nothing here needs an 8 MB
 * source, and decoding one costs real memory in the webview for a result
 * that is thrown away at 64x64. */
const MAX_SOURCE_BYTES = 8 * 1024 * 1024;

/** The largest decoded image accepted, in megapixels.
 *
 * A separate limit from the file size, and the more important of the two: a
 * compressed image is not a bounded thing. A few hundred kilobytes of PNG can
 * decode to hundreds of megapixels - a decompression bomb - and `file.size`
 * says nothing about that. This is the same shape of guard `files::archive`
 * already applies to declared entry sizes in a zip, for the same reason.
 *
 * 40 MP is comfortably above any photograph a person would pick and far below
 * what would hurt. */
const MAX_SOURCE_MEGAPIXELS = 40;

/** A file size a human can read back, for the refusal message. */
function formatBytes(bytes: number): string {
  return bytes >= 1024 * 1024 ? `${(bytes / (1024 * 1024)).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

/** Thrown with a translation key and its interpolation values, so the message
 * the user reads is built by i18next rather than assembled in code. */
class IconRejected extends Error {
  constructor(
    readonly key: string,
    readonly params: Record<string, string | number> = {},
  ) {
    super(key);
  }
}

/**
 * Turns whatever the user picked into a small, safe PNG data URL.
 *
 * The re-encode is the security boundary, not a convenience. The result is
 * written into an `<img src>`, and an SVG in that position can carry script;
 * drawing to a canvas and exporting PNG keeps the pixels and discards
 * everything else - scripts, external references, EXIF, colour profiles.
 * So SVG is refused outright rather than rasterised, because loading it into
 * an `Image` at all is the step worth avoiding.
 *
 * Cropped to a centred square rather than squashed: a wide screenshot squashed
 * into 64x64 is unrecognisable, which defeats the point of having an icon.
 */
async function rasterize(file: File): Promise<string> {
  if (file.type === "image/svg+xml" || file.name.toLowerCase().endsWith(".svg")) {
    throw new IconRejected("serverIcon.noSvg");
  }
  if (file.size > MAX_SOURCE_BYTES) {
    throw new IconRejected("serverIcon.tooLarge", { size: formatBytes(file.size), limit: formatBytes(MAX_SOURCE_BYTES) });
  }

  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(file);
  } catch {
    // A file the browser cannot decode at all - a renamed text file, a
    // truncated download, a format this build has no decoder for.
    throw new IconRejected("serverIcon.notAnImage");
  }
  const megapixels = (bitmap.width * bitmap.height) / 1_000_000;
  if (megapixels > MAX_SOURCE_MEGAPIXELS) {
    bitmap.close();
    throw new IconRejected("serverIcon.tooManyPixels", {
      width: bitmap.width,
      height: bitmap.height,
      limit: MAX_SOURCE_MEGAPIXELS,
    });
  }
  try {
    const canvas = document.createElement("canvas");
    canvas.width = ICON_SIZE;
    canvas.height = ICON_SIZE;
    const context = canvas.getContext("2d");
    if (!context) throw new Error("canvas");

    const edge = Math.min(bitmap.width, bitmap.height);
    const sx = (bitmap.width - edge) / 2;
    const sy = (bitmap.height - edge) / 2;
    context.drawImage(bitmap, sx, sy, edge, edge, 0, 0, ICON_SIZE, ICON_SIZE);
    return canvas.toDataURL("image/png");
  } finally {
    bitmap.close();
  }
}

interface ServerIconPickerProps {
  server: ManagedServer;
  onChanged: (icon: string | null) => void;
}

/**
 * Picking, previewing and clearing a Node's icon.
 *
 * A plain `<input type="file">` rather than Tauri's dialog plugin: it needs no
 * extra capability, it works when the frontend runs outside a Tauri webview
 * (the browser-based UI checks), and the bytes never leave the renderer -
 * they go straight into a canvas.
 */
export function ServerIconPicker({ server, onChanged }: ServerIconPickerProps) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleFile(file: File | undefined) {
    if (!file) return;
    setBusy(true);
    setError(null);
    try {
      const icon = await rasterize(file);
      await setServerIcon(server.id, icon);
      onChanged(icon);
    } catch (err) {
      // A refusal from `rasterize` carries its own key and values; anything
      // else is a real failure from the command and goes through the shared
      // error renderer.
      setError(err instanceof IconRejected ? t(err.key, err.params) : errorMessage(err, t));
    } finally {
      setBusy(false);
      // Cleared so picking the *same* file again still fires a change event.
      if (inputRef.current) inputRef.current.value = "";
    }
  }

  async function clear() {
    setBusy(true);
    setError(null);
    try {
      await setServerIcon(server.id, null);
      onChanged(null);
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="server-icon-picker">
      <div className="server-icon-picker-row">
        <div className="server-icon-picker-preview">
          {server.icon ? (
            <img src={server.icon} alt="" className="server-icon-picker-image" />
          ) : (
            <Icon name={server.connectionMode === "agent" ? "zap" : "server"} size={18} />
          )}
        </div>
        <div className="server-icon-picker-body">
          <p className="server-icon-picker-title">{t("serverIcon.title")}</p>
          <p className="server-icon-picker-hint">{t("serverIcon.hint")}</p>
        </div>
        <div className="server-icon-picker-actions">
          <Button variant="secondary" size="sm" disabled={busy} onClick={() => inputRef.current?.click()}>
            <Icon name="upload" size={14} />
            {busy ? t("common.saving") : t("serverIcon.choose")}
          </Button>
          {server.icon && (
            <Button variant="secondary" size="sm" disabled={busy} onClick={() => void clear()}>
              {t("serverIcon.clear")}
            </Button>
          )}
        </div>
      </div>
      <input
        ref={inputRef}
        type="file"
        accept={ACCEPTED}
        className="server-icon-picker-input"
        onChange={(event) => void handleFile(event.target.files?.[0])}
      />
      {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
    </div>
  );
}
