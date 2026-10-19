// 附件 (Rust src-tauri/src/modules/attachments.rs): files hung on notes, tasks and goals. Adding stores
// a copy first (not undoable); the record's save writes which files it has (one ⌘Z step).
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export type Owner = "note" | "task" | "goal";
/** kind: image | file. missing: not on this Mac (from a data file, or cleaned up). */
export type FileInfo = { id: number; name: string; kind: "image" | "file"; bytes: number; missing: boolean };
export type AttachmentUsage = { files: number; bytes: number; unused: number; unusedBytes: number; dir: string };

export const attachmentsApi = {
  addPaths: (paths: string[]) => invoke<FileInfo[]>("attachments_add_paths", { paths }),
  /** Pasted / dropped bytes; the name travels percent-encoded in a header. */
  addFile: async (file: File) =>
    invoke<FileInfo>("attachments_add_bytes", new Uint8Array(await file.arrayBuffer()), { headers: { "x-name": encodeURIComponent(file.name || "粘贴的图片.png") } }),
  of: (owner: Owner, record: number) => invoke<FileInfo[]>("attachments_of", { owner, record }),
  open: (id: number) => invoke<void>("attachment_open", { id }),
  usage: () => invoke<AttachmentUsage>("attachments_usage"),
  clean: () => invoke<number>("attachments_clean"),
};

export const ATTACHMENTS_KEY = ["attachments"];
export const useAttachmentsOf = (owner: Owner, record: number | null) =>
  useQuery({ queryKey: [...ATTACHMENTS_KEY, owner, record], queryFn: () => attachmentsApi.of(owner, record!), enabled: record !== null, gcTime: 0 });

/** Where the page loads a stored image (lib.rs serves it; the browser mock hands out its own URLs). */
export function fileSrc(id: number): string {
  const mock = (window as unknown as { __pomeloFileSrc?: (id: number) => string }).__pomeloFileSrc;
  return mock ? mock(id) : `pomelo-file://localhost/${id}`;
}

export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(bytes < 10 * 1024 * 1024 ? 1 : 0)} MB`;
}
