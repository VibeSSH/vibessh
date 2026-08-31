use serde::{Deserialize, Serialize};

/// What Desktop wants a Node to converge on - Etap M3 introduces the
/// channel this travels over and the revisioning around it, but carries no
/// real payload yet (no firewall rules, WireGuard config, or DNS fragments
/// exist to push - those are later, deferred phases, see
/// docs/APPLICATIONS_ARCHITECTURE.md's own Etap M roadmap notes). An empty
/// struct rather than `()` so adding real fields later is a
/// backward-compatible schema change (`#[serde(default)]` per field). An
/// Agent applying an empty state always succeeds - there is nothing yet
/// that could fail.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDesiredState {}

/// Every message Desktop can push to an Agent, over the same WebSocket a
/// `ServerEvent` already flows the other way on - mirrors that type's own
/// internally-tagged, dot-notation shape (`{"type":"state.apply",...}`) so
/// the wire format stays symmetric in both directions. Adding a future
/// command (Etap M6+: firewall/WireGuard/DNS pushes) is one more variant
/// here, not a new channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DesktopCommand {
    #[serde(rename = "state.apply")]
    ApplyDesiredState { revision: u64, state: NodeDesiredState },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_desired_state_round_trips_through_json_with_the_dot_notation_tag() {
        let command = DesktopCommand::ApplyDesiredState { revision: 7, state: NodeDesiredState::default() };
        let json = serde_json::to_value(&command).unwrap();
        assert_eq!(json["type"], "state.apply");
        assert_eq!(json["revision"], 7);

        let round_tripped: DesktopCommand = serde_json::from_value(json).unwrap();
        let DesktopCommand::ApplyDesiredState { revision, .. } = round_tripped;
        assert_eq!(revision, 7);
    }
}
