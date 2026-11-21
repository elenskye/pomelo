//! 助手 (v0.4.5 阶段 8): small agents 柚子 provides, each with its own page (not a conversation), its own
//! way of working, prompt and settings. Listed in 柚子's left column between 项目 and 定时 (the page side
//! keeps the list: `src/modules/youzi/assistants/`). Built in: 翻译, Pub2MD, Seeker, Rater and Importer (its page is
//! 钱来钱往 · 导入账单).
//! - 翻译 and Pub2MD keep settings and last work in youzi.db `preferences` (`assistant.<key>`), never undone and
//!   never exported. Model work goes through the same 柚子服务 as everything else (schedule.rs `route`
//!   for its own model choice, provider.rs for the call), without taking a conversation's slot.
//! - Seeker keeps its workspace in its own youzi.db tables, with undo and data-file export.

pub mod importer;
pub mod janitor;
pub mod pub2md;
pub mod rater;
pub mod seeker;
pub mod site;
pub mod translate;

/// A function's model and 思考 on an assistant's settings page (Rater's, Seeker's).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Choice {
    pub model: Option<String>,
    pub thinking: bool,
    /// Its 思考强度 (None: the model's default).
    pub effort: Option<String>,
}

impl Choice {
    pub fn level(&self) -> super::cloud::Level {
        super::cloud::Level::of(self.thinking, self.effort.as_deref())
    }
}

/// What the assistants are doing right now (in memory).
#[derive(Default)]
pub struct State {
    pub translate: translate::Live,
    /// 呦呦漫游's 按中文起草英文版 (the same pipeline as 翻译).
    pub site: site::Live,
    pub pub2md: pub2md::Live,
}
