// 柚子 in 见字如面 in the browser mock (?mock): 润色 a passage, 续写 at the caret, answered like the mock model does in
// Rust (youzi/assist.rs `mock_reply`). Spread into mockIpc.ts's handlers; the articles mock may set
// `youziArticlesMock.article` so a locked article or one in the trash is refused as Rust refuses it.
type Args = Record<string, unknown>;

export const youziArticlesMock: { article?: (id: number) => { locked?: boolean; trashed?: boolean } | undefined } = {};

export const youziArticlesHandlers: Record<string, (a: Args) => unknown> = {
  article_assist: async (a) => {
    await new Promise((r) => setTimeout(r, 700));
    const mode = String(a.mode ?? "");
    const text = String(a.text ?? "");
    const after = String(a.after ?? "");
    if (mode !== "polish" && mode !== "continue") throw `不认识的帮忙：${mode}`;
    const article = youziArticlesMock.article?.(Number(a.id));
    if (youziArticlesMock.article && !article) throw "找不到这篇文章，可能已经被删除了";
    if (article?.locked) throw "这篇加密了：柚子不看加密的文章";
    if (article?.trashed) throw "这篇在废纸篓里：先捡回来再让柚子帮忙";
    if (mode === "polish") {
      if (!text.trim()) throw "没有可以润色的文字";
      return { text: `${text.trim().replace(/很/g, "非常")}（模拟润色）` };
    }
    if (!text.trim() && !after.trim()) throw "先写几句，柚子再接着写";
    return { text: "接着写下去的一段。（模拟续写）" };
  },
};
