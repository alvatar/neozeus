use super::{AegisCommand, AppCommand, ComposerCommand, ComposerRequest};
use crate::{agents::AgentId, composer::ComposerMode};

/// Verifies that app command can wrap composer request.
#[test]
fn app_command_can_wrap_composer_request() {
    let command = AppCommand::Composer(ComposerCommand::Open(ComposerRequest {
        mode: ComposerMode::Message {
            agent_id: AgentId(1),
        },
    }));
    assert!(matches!(command, AppCommand::Composer(_)));
}

#[test]
fn app_command_can_wrap_aegis_request() {
    let command = AppCommand::Aegis(AegisCommand::Enable {
        agent_id: AgentId(1),
        prompt_text: "keep going".into(),
    });
    assert!(matches!(command, AppCommand::Aegis(_)));
}
