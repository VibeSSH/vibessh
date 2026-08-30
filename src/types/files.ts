/** Mirrors the Rust `RemoteFileEntry` DTO. */
export interface RemoteFileEntry {
  name: string;
  path: string;
  isDir: boolean;
  isSymlink: boolean;
  size: number;
  modifiedAt?: string;
}
