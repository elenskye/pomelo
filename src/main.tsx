import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter, Navigate, Route, Routes } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { App } from "@/app/App";
import BooksPage from "@/modules/books/BooksPage";
import FilmsPage from "@/modules/films/FilmsPage";
import DrawingPage from "@/modules/drawing/DrawingPage";
import PackingPage from "@/modules/packing/PackingPage";
import HomePage from "@/home/HomePage";
import FinancePage from "@/modules/finance/FinancePage";
import JobsPage from "@/modules/jobs/JobsPage";
import NotesPage from "@/modules/notes/NotesPage";
import VaultPage from "@/modules/vault/VaultPage";
import TodoPage from "@/modules/todo/TodoPage";
import SettingsPage from "@/settings/SettingsPage";
import { ToastProvider } from "@/shared/feedback";
import "./index.css";

// Browser-only UI testing: `pnpm dev` → http://localhost:1420/?mock (tree-shaken from builds).
if (import.meta.env.DEV && new URLSearchParams(location.search).has("mock")) {
  await (await import("./dev/mockIpc")).installMockIpc();
}

// Local IPC: no network retries / refetch-on-focus noise.
const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, staleTime: Infinity } },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
      <HashRouter>
        <Routes>
          <Route element={<App />}>
            <Route index element={<HomePage />} />
            <Route path="todo" element={<TodoPage />} />
            <Route path="books" element={<BooksPage />} />
            <Route path="films" element={<FilmsPage />} />
            <Route path="drawing" element={<DrawingPage />} />
            <Route path="packing" element={<PackingPage />} />
            <Route path="finance" element={<FinancePage />} />
            <Route path="jobs" element={<JobsPage />} />
            <Route path="notes" element={<NotesPage />} />
            <Route path="vault" element={<VaultPage />} />
            <Route path="settings" element={<SettingsPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
      </HashRouter>
      </ToastProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
