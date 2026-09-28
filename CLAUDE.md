# CLAUDE.md

Pomelo OS（柚儿园）：只给我一个人用的 macOS 本地个人系统。技术栈是 Tauri 2 + React + TypeScript + Rust + SQLite。

## 开工前先读

1. `docs/roadmap.md`：现在做到哪一步、下一步做什么
2. 按需再读：`docs/architecture.md`（架构）、`docs/schema.md`（数据结构）、`docs/design.md`（视觉）

## 最高原则

- **个人、轻量**：只在一台 Mac 上给一个人用。不做账号、多人协作、云端服务、跨平台兼容，不搭复杂架构，不做「以后可能用得上」的抽象。
- **按阶段推进**：只做当前阶段的事。每个阶段结束时功能必须真正能用，并说明验收结果。
- **重要决定先问我**：数据结构、页面结构、新增依赖、功能范围。给出推荐方案，由我来定。

## 数据规则

- 只有 Rust 端读写 SQLite。界面（以及以后的 AI 助手）只调用 Rust 命令，不写 SQL。
- 业务规则写在 Rust 命令层，只写一处。
- 改数据库结构只能新增 migration 脚本；已有的 migration 永远不改。
- 开发和测试只用开发数据目录（`Pomelo OS Dev`）。除非我明确要求，绝不读写真实数据目录 `~/Library/Application Support/Pomelo OS/`。
- 个人数据（xlsx、数据库、备份、导出文件）不进 Git。`prototype/柚儿园.xlsx` 只读，不修改。

## 文档规则

- **给我看的**：根目录的 `README.md`、`CLAUDE.md` 和 `docs/` 下的文档。要简洁、通俗、全中文。
- **不给我看的**：代码、注释、脚本说明、提交信息。我不做人工审查，按 Agent 最容易理解、效率最高的方式写就行。
- 每个阶段完成后更新 `docs/roadmap.md`。数据结构变了同步改 `docs/schema.md`，视觉变了同步改 `docs/design.md`。

## 环境

- macOS，Apple M3，18 GB 内存
- Node 24、pnpm、Rust、Xcode 命令行工具都已安装。pnpm 在 `~/Library/pnpm/bin`，cargo 在 `~/.cargo/bin`；非交互终端里要先把它们加进 PATH
- 辅助脚本用 TypeScript（Node），不引入 Python

## 常用命令

| 命令 | 作用 |
|---|---|
| `pnpm app:dev` | 运行开发版（测试数据） |
| `pnpm check` | 类型检查 + Rust 自动测试，每次改完都要跑 |
| `pnpm app:build` | 只打包正式版，不安装 |
| `pnpm app:install` | 打包并安装到「应用程序」，只在阶段验收后、我同意时执行 |

## 验证方法

- 开发版的窗口无法被后台截图。需要截图时，先 `pnpm app:build`，再用 `open -n --env POMELO_DATA_DIR=<临时目录> "src-tauri/target/release/bundle/macos/Pomelo OS.app"` 打开，这样不会碰真实数据。
- 想打开某一页，就把临时数据库 `settings` 表里 `ui.last_route` 的值改成那一页（如 `"/settings"`），再重开 App。
