import { callCommand } from "./tauri";
import type { RemoteFileEntry } from "@/types/files";

export function listRemoteDirectory(serverId: string, path: string): Promise<RemoteFileEntry[]> {
  return callCommand<RemoteFileEntry[]>("list_remote_directory", { serverId, path });
}

export function createRemoteDirectory(serverId: string, path: string): Promise<void> {
  return callCommand<void>("create_remote_directory", { serverId, path });
}

/** Bytes come back as a plain number array over Tauri IPC - see bytesToText for decoding. */
export function readRemoteFile(serverId: string, path: string): Promise<number[]> {
  return callCommand<number[]>("read_remote_file", { serverId, path });
}

export function writeRemoteFile(serverId: string, path: string, contents: number[]): Promise<void> {
  return callCommand<void>("write_remote_file", { serverId, path, contents });
}

/** Streams straight between the remote file and a local path chosen via a native save dialog - never round-trips the file's bytes through this JS layer. */
export function downloadRemoteFile(serverId: string, remotePath: string, localPath: string): Promise<void> {
  return callCommand<void>("download_remote_file", { serverId, remotePath, localPath });
}

/** The upload counterpart of downloadRemoteFile - localPath comes from a native open-file dialog. */
export function uploadRemoteFile(serverId: string, localPath: string, remotePath: string): Promise<void> {
  return callCommand<void>("upload_remote_file", { serverId, localPath, remotePath });
}

/** Covers both "Rename" (same parent, new name) and "Move" (new parent) - the same underlying primitive either way, the caller just builds a different `to`. */
export function renameRemotePath(serverId: string, from: string, to: string): Promise<void> {
  return callCommand<void>("rename_remote_path", { serverId, from, to });
}

/** Recursive for a directory. */
export function deleteRemotePath(serverId: string, path: string): Promise<void> {
  return callCommand<void>("delete_remote_path", { serverId, path });
}

/** Extracts an already-present `.zip` at `archivePath` into `destination` - resolves to the number of files written. */
export function extractRemoteArchive(serverId: string, archivePath: string, destination: string): Promise<number> {
  return callCommand<number>("extract_remote_archive", { serverId, archivePath, destination });
}

/** Compresses `paths` into a new `.zip` written to `destinationPath`. */
export function compressRemotePaths(serverId: string, paths: string[], destinationPath: string): Promise<void> {
  return callCommand<void>("compress_remote_paths", { serverId, paths, destinationPath });
}

export function bytesToText(bytes: number[]): string {
  return new TextDecoder().decode(new Uint8Array(bytes));
}

export function textToBytes(text: string): number[] {
  return Array.from(new TextEncoder().encode(text));
}
