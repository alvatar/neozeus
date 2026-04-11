#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgentDurability {
    Recoverable,
    #[default]
    LiveOnly,
}
