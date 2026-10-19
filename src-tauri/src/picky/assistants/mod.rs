//! 助手 (v0.4.5 阶段 8): small agents 小拾 provides, each with its own page (not a conversation), its own
//! way of working, prompt and settings. Listed in 小拾's left column between 项目 and 定时 (the page side
//! keeps the list: `src/modules/picky/assistants/`). Built in only: 翻译 now; Work next; Pub2MD later.
//! - Each keeps its settings and last work in picky.db `preferences` (`assistant.<key>`), never undone and
//!   never exported. Model work goes through the same 小拾服务 as everything else (schedule.rs `route`
//!   for its own model choice, provider.rs for the call), without taking a conversation's slot.

pub mod translate;

/// What the assistants are doing right now (in memory).
#[derive(Default)]
pub struct State {
    pub translate: translate::Live,
}
