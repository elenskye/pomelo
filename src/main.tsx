import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter, Navigate, Route, Routes } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { App } from "@/app/App";
import { DEFAULT_ROUTE } from "@/app/modules";
import BooksPage from "@/modules/books/BooksPage";
import FilmsPage from "@/modules/films/FilmsPage";
import DrawingPage from "@/modules/drawing/DrawingPage";
import PackingPage from "@/modules/packing/PackingPage";
import SettingsPage from "@/settings/SettingsPage";
import "./index.css";

// Local IPC: no network retries / refetch-on-focus noise.
const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, staleTime: Infinity } },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <HashRouter>
        <Routes>
          <Route element={<App />}>
            <Route index element={null} />
            <Route path="books" element={<BooksPage />} />
            <Route path="films" element={<FilmsPage />} />
            <Route path="drawing" element={<DrawingPage />} />
            <Route path="packing" element={<PackingPage />} />
            <Route path="settings" element={<SettingsPage />} />
            <Route path="*" element={<Navigate to={DEFAULT_ROUTE} replace />} />
          </Route>
        </Routes>
      </HashRouter>
    </QueryClientProvider>
  </React.StrictMode>,
);
