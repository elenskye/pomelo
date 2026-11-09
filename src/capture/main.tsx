// The 速记小窗's page (capture.html; window `capture`, made by Rust capture.rs).
import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { startAppearance } from "@/shared/appearance";
import { startHand } from "@/shared/hand";
import { startTips } from "@/shared/tips";
import { ToastProvider } from "@/shared/feedback";
import { Capture } from "./Capture";
import "@/index.css";

// Browser-only UI testing: `pnpm dev` → http://localhost:1420/capture.html?mock
if (import.meta.env.DEV && new URLSearchParams(location.search).has("mock")) {
  await (await import("@/dev/mockIpc")).installMockIpc("capture");
}
// the palette and light / dark chosen in 设置 (re-read whenever the window comes back)
startAppearance();
// the hand-drawn frames and controls (data-hand), as in the main window
startHand();
startTips();

// as the main window's (问柚子 uses 柚子's queries)
const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, staleTime: Infinity } },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <Capture />
      </ToastProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
