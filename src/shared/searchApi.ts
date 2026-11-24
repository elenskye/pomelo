// Search (v0.4): typed wrappers for src-tauri/src/search (query.rs, indexer.rs). Rules live in Rust.
import { invoke } from "@tauri-apps/api/core";

type IndexStatus = {
  records: number;
  done: number;
  rebuilding: boolean;
  filling: boolean;
  model: string | null;
  bytes: number;
  error: string | null;
};

export const searchApi = {
  indexStatus: () => invoke<IndexStatus>("search_index_status"),
};
