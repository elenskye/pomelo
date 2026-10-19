// Search (v0.4): typed wrappers for src-tauri/src/search (query.rs, indexer.rs). Rules live in Rust.
import { invoke } from "@tauri-apps/api/core";

export type SearchWay = "words" | "meaning" | "both";

export type SearchRequest = {
  text: string;
  way: SearchWay;
  /** Empty: every module (hidden ones and 芝麻开门 never). */
  modules?: string[];
  kinds?: string[];
  /** The results go to a cloud model. */
  forCloud?: boolean;
  limit?: number;
};

export type SearchFound = {
  module: string;
  kind: string;
  id: number;
  title: string;
  detail: string | null;
  parent: number | null;
  day: string | null;
  snippet: string;
  score: number;
  by: SearchWay;
};

export type SearchAnswer = { found: SearchFound[]; note: string | null };

export type IndexStatus = {
  records: number;
  done: number;
  rebuilding: boolean;
  filling: boolean;
  model: string | null;
  bytes: number;
  error: string | null;
};

export const searchApi = {
  query: (request: SearchRequest) => invoke<SearchAnswer>("search_query", { request }),
  indexStatus: () => invoke<IndexStatus>("search_index_status"),
};
