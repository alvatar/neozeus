use bevy::prelude::Resource;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ClaudeUsageData {
    pub(crate) session_pct: f32,
    pub(crate) week_pct: f32,
    pub(crate) extra_pct: f32,
    pub(crate) extra_used: f32,
    pub(crate) extra_limit: f32,
    pub(crate) session_resets_at: String,
    pub(crate) week_resets_at: String,
    pub(crate) available: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OpenAiUsageData {
    pub(crate) requests_pct_milli: i32,
    pub(crate) tokens_pct_milli: i32,
    pub(crate) requests_limit: i32,
    pub(crate) requests_remaining: i32,
    pub(crate) tokens_limit: i32,
    pub(crate) tokens_remaining: i32,
    pub(crate) requests_resets_at: String,
    pub(crate) tokens_resets_at: String,
    pub(crate) available: bool,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct UsageSnapshot {
    pub(crate) claude: ClaudeUsageData,
    pub(crate) openai: OpenAiUsageData,
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct UsagePersistenceState {
    pub(crate) state_dir: PathBuf,
    pub(crate) claude_cache_path: PathBuf,
    pub(crate) openai_cache_path: PathBuf,
    pub(crate) claude_log_path: PathBuf,
    pub(crate) openai_log_path: PathBuf,
    pub(crate) claude_backoff_until_path: PathBuf,
    pub(crate) claude_refresh_lock_path: PathBuf,
    pub(crate) openai_refresh_lock_path: PathBuf,
    pub(crate) helper_script_path: PathBuf,
    pub(crate) python_program: PathBuf,
    pub(crate) last_claude_refresh_attempt_secs: Option<f32>,
    pub(crate) last_openai_refresh_attempt_secs: Option<f32>,
}
