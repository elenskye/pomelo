import React, { Suspense, lazy } from "react";
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
import DaysPage from "@/modules/days/DaysPage";
import SitePage from "@/modules/site/SitePage";
import ListPage from "@/modules/custom/ListPage";
import ProgressPage from "@/modules/custom/ProgressPage";
import VaultPage from "@/modules/vault/VaultPage";
import BookmarksPage from "@/modules/bookmarks/BookmarksPage";
// 柚子 brings Markdown / highlighting / formulas: loaded when first opened
const YouziPage = lazy(() => import("@/modules/youzi/YouziPage"));
// 柚子's 助手 as modules (v0.7)
const AssistantModule = lazy(() => import("@/modules/youzi/assistants"));
import DockPage from "@/app/DockPage";
import TodoPage from "@/modules/todo/TodoPage";
import HealthPage from "@/settings/Health";
import SettingsPage from "@/settings/SettingsPage";
import { startAppearance } from "@/shared/appearance";
import { ToastProvider } from "@/shared/feedback";
import "./index.css";

// Browser-only UI testing: `pnpm dev` → http://localhost:1420/?mock (tree-shaken from builds).
if (import.meta.env.DEV && new URLSearchParams(location.search).has("mock")) {
  await (await import("./dev/mockIpc")).installMockIpc();
}
// the palette and light / dark chosen in 设置, before the first paint
startAppearance();

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
            <Route path="finance/:sub?" element={<FinancePage />} />
            <Route path="jobs/:sub?" element={<JobsPage />} />
            <Route path="notes" element={<NotesPage />} />
            <Route path="days" element={<DaysPage />} />
            <Route path="site/:sub?" element={<SitePage />} />
            <Route path="list/:id" element={<ListPage />} />
            <Route path="progress/:id" element={<ProgressPage />} />
            <Route path="vault" element={<VaultPage />} />
            <Route path="bookmarks" element={<BookmarksPage />} />
            <Route path="youzi" element={<Suspense fallback={null}><YouziPage /></Suspense>} />
            <Route path="translate/:sub?" element={<Suspense fallback={null}><AssistantModule which="translate" /></Suspense>} />
            <Route path="pub2md/:sub?" element={<Suspense fallback={null}><AssistantModule which="pub2md" /></Suspense>} />
            <Route path="seeker/:sub?" element={<Suspense fallback={null}><AssistantModule which="seeker" /></Suspense>} />
            <Route path="rater/:sub?" element={<Suspense fallback={null}><AssistantModule which="rater" /></Suspense>} />
            <Route path="janitor/:sub?" element={<Suspense fallback={null}><AssistantModule which="janitor" /></Suspense>} />
            <Route path="importer" element={<Navigate to="/finance/import" replace />} />
            <Route path="dock" element={<DockPage />} />
            <Route path="settings" element={<SettingsPage />} />
            <Route path="settings/health" element={<HealthPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
      </HashRouter>
      </ToastProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
