# AGENTS.md

给 Codex 等 Agent 的入口。规则、代码地图和验证方法只写在 [CLAUDE.md](CLAUDE.md) 一处，对你同样适用（里面说「你（Claude）」的地方就是指你）；开工前先读它和 `docs/roadmap.md`。

- 在单独的 worktree 里干活：照 CLAUDE.md 的「多个 worktree」，开发数据目录用自己的，不提交、不合并、不推送。
- 做 Seeker 相关的：照 CLAUDE.md 的「Seeker」和 `docs/roadmap.md` 的「v0.7」。Codex 的第一轮（Seeklo）只写代码：不跑构建、类型检查和测试，不用任何数据（包括编的测试数据），不读写真的 Seeker 文件夹。验证都交给 Claude。
- 做完照 CLAUDE.md 的「汇报」写，再列出公共接口、存储的变化和要 Claude 验证的事，Claude 会照着审核。
