use super::{AppCommand, ComposerCommand, ComposerRequest};
use crate::{agents::AgentId, ui::ComposerMode};

#[test]
fn app_command_can_wrap_composer_request() {
    let command = AppCommand::Composer(ComposerCommand::Open(ComposerRequest {
        mode: ComposerMode::Message {
            agent_id: AgentId(1),
        },
    }));
    assert!(matches!(command, AppCommand::Composer(_)));
}
