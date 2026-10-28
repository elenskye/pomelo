//! 助手 (v0.4.5 阶段 8): small agents 小拾 provides, each with its own page (not a conversation), its own
//! way of working, prompt and settings. Listed in 小拾's left column between 项目 and 定时 (the page side
//! keeps the list: `src/modules/picky/assistants/`). Built in: 翻译, Pub2MD and Seeker.
//! - 翻译 and Pub2MD keep settings and last work in picky.db `preferences` (`assistant.<key>`), never undone and
//!   never exported. Model work goes through the same 小拾服务 as everything else (schedule.rs `route`
//!   for its own model choice, provider.rs for the call), without taking a conversation's slot.
//! - Seeker keeps its workspace in its own picky.db tables, with undo and data-file export.

pub mod pub2md;
pub mod rater;
pub mod seeker;
pub mod site;
pub mod translate;

/// What the assistants are doing right now (in memory).
#[derive(Default)]
pub struct State {
    pub translate: translate::Live,
    /// 呦呦漫游's 按中文起草英文版 (the same pipeline as 翻译).
    pub site: site::Live,
    pub pub2md: pub2md::Live,
}
