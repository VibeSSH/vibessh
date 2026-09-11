/** Mirrors the Rust `RemoteFileEntry` DTO. */
export interface RemoteFileEntry {
  name: string;
  path: string;
  isDir: boolean;
  isSymlink: boolean;
  size: number;
  modifiedAt?: string;
  /** POSIX mode bits (e.g. 0o755), when the source actually reports them - absent on a provider with no meaningful concept of Unix permissions (Local on Windows, for one). */
  permissions?: number;
}
