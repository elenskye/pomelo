# AGENTS.md

给 Codex 等 Agent 的入口。规则和代码地图只写在 [CLAUDE.md](CLAUDE.md) 一处、怎么测在 `docs/testing.md`，对你同样适用（里面说「你（Claude）」的地方就是指你）；开工前先读它和 `docs/roadmap.md`。

- 在主目录干活（不开 worktree，见 CLAUDE.md 的「只在主目录做」）：开发数据目录用自己的（`POMELO_DATA_DIR`），不提交、不合并、不推送；不和 Claude 同时改同一批文件。
- 做完照 CLAUDE.md 的「汇报」写，再列出公共接口、存储的变化和要 Claude 验证的事，Claude 会照着审核。
