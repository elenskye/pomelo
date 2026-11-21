// 设置 → 呦呦漫游: which folder is the website (checked in Rust: a git repo with package.json).
import { open } from "@tauri-apps/plugin-dialog";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { OVERVIEW_KEY, SITE_KEY, siteApi } from "@/modules/site/api";
import { errorText, useToast } from "@/shared/feedback";
import { Button } from "@/shared/ui";
import { Path, Row, Section } from "./layout";

export function SiteSection() {
  const qc = useQueryClient();
  const toast = useToast();
  const { data: o } = useQuery({ queryKey: OVERVIEW_KEY, queryFn: siteApi.overview });
  if (!o) return null;
  const set = (folder: string | null) =>
    void siteApi.setFolder(folder).then(
      (v) => {
        qc.setQueryData(OVERVIEW_KEY, v);
        void qc.invalidateQueries({ queryKey: SITE_KEY, predicate: (q) => q.queryKey[1] !== "overview" });
      },
      (e) => toast(errorText(e), "error"),
    );
  const pick = async () => {
    const p = await open({ directory: true, multiple: false, title: "网站文件夹", defaultPath: o.folder });
    if (typeof p === "string") set(p);
  };
  const help =
    o.mode === "sandbox"
      ? "开发版默认用开发数据目录里的副本（在呦呦漫游里点「建副本」），只推到旁边的假远端，不会推到真的 GitHub。"
      : "呦呦漫游编辑、发布的就是这个文件夹里的网站。它是独立的项目，有自己的 git 历史；发布时推到它的 GitHub。";
  return (
    <Section title="呦呦漫游">
      <Row
        label="网站文件夹"
        help={help}
        actions={
          <>
            {!o.isDefault && <Button onClick={() => set(null)}>恢复默认</Button>}
            <Button onClick={() => void pick()}>选择…</Button>
          </>
        }
      >
        <Path value={o.folder} />
      </Row>
    </Section>
  );
}
