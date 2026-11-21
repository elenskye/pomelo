# AGENTS.md

给 Codex 等 Agent 的入口。规则和代码地图只写在 [CLAUDE.md](CLAUDE.md) 一处（里面的「你」也指你），怎么测在 `docs/testing.md`；开工前照它的「开工前」来。

- 在主目录干活（不开 worktree）：开发数据目录用自己的（`POMELO_DATA_DIR`），不提交、不合并、不推送；不和 Claude 同时改同一批文件。
- 做完照 CLAUDE.md 的「汇报」写，再列出公共接口、存储的变化和要 Claude 验证的事，Claude 会照着审核。
