// Conversation actions shared by the list's ⋯ menu and the batch bar: rename / pin / archive / move /
// delete / copy / export. Rules live in Rust (library.rs, export.rs); every change here can be undone (⌘Z).
import { useQueryClient } from "@tanstack/react-query";
import { save } from "@tauri-apps/plugin-dialog";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { LIBRARY_KEY, pickyApi } from "./api";

const fileName = (t: string) => t.replace(/[/\\:*?"<>|\n]/g, " ").trim().slice(0, 60) || "对话";

export function useConversationActions() {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
    void qc.invalidateQueries({ queryKey: ["picky", "sessions"] });
  };
  const run = async (f: () => Promise<unknown>, done?: string) => {
    try {
      await f();
      refresh();
      if (done) toast(done);
      return true;
    } catch (e) {
      toast(errorText(e), "error");
      return false;
    }
  };
  return {
    confirm,
    confirmDialog,
    rename: (id: number, title: string) => run(() => pickyApi.rename(id, title)),
    pin: (ids: number[], pinned: boolean) => run(() => pickyApi.pin(ids, pinned)),
    archive: (ids: number[], archived: boolean) => run(() => pickyApi.archive(ids, archived)),
    setPrivate: (ids: number[], priv: boolean) => run(() => pickyApi.setPrivate(ids, priv)),
    move: (ids: number[], projectId: number | null) => run(() => pickyApi.move(ids, projectId)),
    remove: async (ids: number[], title?: string) => {
      const ok = await confirm({
        title: ids.length === 1 ? `删除「${title ?? "这个对话"}」？` : `删除选中的 ${ids.length} 个对话？`,
        message: "里面的消息会一起删掉。",
        confirmLabel: "删除",
        danger: true,
      });
      return ok && run(() => pickyApi.remove(ids));
    },
    copy: (id: number) => run(() => pickyApi.copyConversation(id), "已复制整段对话"),
    exportMarkdown: async (id: number, title: string) => {
      const path = await save({ defaultPath: `${fileName(title)}.md`, filters: [{ name: "Markdown", extensions: ["md"] }] });
      if (path) await run(() => pickyApi.exportMarkdown(id, path), "已导出");
    },
    exportPackage: async (id: number, title: string) => {
      const path = await save({ defaultPath: fileName(title) });
      if (path) await run(() => pickyApi.exportPackage(id, path), "已导出完整包");
    },
  };
}
