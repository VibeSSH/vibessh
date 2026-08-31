import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { callCommand } from "./tauri";
import type { RemoteFileEntry } from "@/types/files";
import type { FileHistoryVersion, TransferProgressEvent } from "@/types/applicationFiles";

export function listApplicationFiles(applicationId: string, path: string): Promise<RemoteFileEntry[]> {
  return callCommand<RemoteFileEntry[]>("list_application_files", { applicationId, path });
}

export function getApplicationFileMetadata(applicationId: string, path: string): Promise<RemoteFileEntry> {
  return callCommand<RemoteFileEntry>("get_application_file_metadata", { applicationId, path });
}

/** Bytes come back as a plain number array over Tauri IPC - see filesService's bytesToText for decoding (reused as-is, it's a generic byte<->text helper, not Node-Files-specific). */
export function readApplicationFile(applicationId: string, path: string): Promise<number[]> {
  return callCommand<number[]>("read_application_file", { applicationId, path });
}

/** Plain create-or-truncate write - "New File" and similar. Not the editor's own Save (see saveApplicationFile). */
export function writeApplicationFile(applicationId: string, path: string, contents: number[]): Promise<void> {
  return callCommand<void>("write_application_file", { applicationId, path, contents });
}

export function saveApplicationFile(applicationId: string, path: string, contents: number[], backup: boolean): Promise<void> {
  return callCommand<void>("save_application_file", { applicationId, path, contents, backup });
}

export function createApplicationDirectory(applicationId: string, path: string): Promise<void> {
  return callCommand<void>("create_application_directory", { applicationId, path });
}

export function deleteApplicationFile(applicationId: string, path: string): Promise<void> {
  return callCommand<void>("delete_application_file", { applicationId, path });
}

/** Covers both "Rename" (same directory, new name) and "Move" (new directory) - the frontend just builds a different `to` path for each. */
export function renameApplicationFile(applicationId: string, from: string, to: string): Promise<void> {
  return callCommand<void>("rename_application_file", { applicationId, from, to });
}

export function copyApplicationFile(applicationId: string, from: string, to: string): Promise<void> {
  return callCommand<void>("copy_application_file", { applicationId, from, to });
}

export function setApplicationFilePermissions(applicationId: string, path: string, mode: number): Promise<void> {
  return callCommand<void>("set_application_file_permissions", { applicationId, path, mode });
}

/** `localDest` comes from a native save dialog - streams directly backend<->filesystem, never round-trips bytes through this JS layer. Progress arrives via onTransferProgress(transferId, ...) while this promise is still pending. */
export function downloadApplicationFile(applicationId: string, path: string, localDest: string, transferId: string): Promise<void> {
  return callCommand<void>("download_application_file", { applicationId, path, localDest, transferId });
}

/** `localSrc` comes from a native open-file dialog. */
export function uploadApplicationFile(applicationId: string, localSrc: string, path: string, transferId: string): Promise<void> {
  return callCommand<void>("upload_application_file", { applicationId, localSrc, path, transferId });
}

/** `true` if a running transfer was actually found and aborted - `false` means it already finished. */
export function cancelApplicationFileTransfer(transferId: string): Promise<boolean> {
  return callCommand<boolean>("cancel_application_file_transfer", { transferId });
}

/** Extracts an archive that's already in the Application's own file tree (uploaded the normal, streaming way) - never round-trips its bytes through this JS layer a second time. */
export function extractApplicationArchive(applicationId: string, sourcePath: string, destination: string): Promise<number> {
  return callCommand<number>("extract_application_archive", { applicationId, sourcePath, destination });
}

export function listApplicationFileHistory(applicationId: string, path: string): Promise<FileHistoryVersion[]> {
  return callCommand<FileHistoryVersion[]>("list_application_file_history", { applicationId, path });
}

export function restoreApplicationFileHistory(applicationId: string, path: string, timestamp: string): Promise<void> {
  return callCommand<void>("restore_application_file_history", { applicationId, path, timestamp });
}

/** Same not-in-a-Tauri-webview guard as pairingService/terminalService's own event helpers. */
export function onTransferProgress(transferId: string, handler: (progress: TransferProgressEvent) => void): Promise<UnlistenFn> {
  return listen<TransferProgressEvent>(`application-files://${transferId}/progress`, (event) => handler(event.payload)).catch(() => () => {});
}
