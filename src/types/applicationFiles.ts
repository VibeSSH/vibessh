export type { RemoteFileEntry } from "./files";

export interface FileHistoryVersion {
  timestamp: string;
  size: number;
}

export interface TransferProgressEvent {
  transferred: number;
  total: number;
}
