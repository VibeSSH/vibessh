use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

    /// Start streaming a container's output back as `ServerEvent::LogsLine`.
    ///
    /// `follow_id` is the Desktop's, and every line comes back carrying it:
    /// one Node can have several consoles open at once - two Applications,
    /// or the same one in two windows - over the single connection this
    /// protocol has, so the lines have to be separable at the other end.
    ///
    /// The container is named rather than the Application: the Agent has no
    /// database and does not know what an Application is. Deciding which
    /// container an Application means is the Desktop's job, the same
    /// division `state.apply` already follows.
    #[serde(rename = "logs.follow")]
    FollowLogs { follow_id: Uuid, container: String, tail: u32 },

    /// Stop a stream started by `FollowLogs`. Unknown ids are ignored - a
    /// console closing twice, or after a reconnect that already dropped
    /// the follow, is a race rather than an error.
    #[serde(rename = "logs.stop")]
    StopFollowingLogs { follow_id: Uuid },
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
        match round_tripped {
            DesktopCommand::ApplyDesiredState { revision, .. } => assert_eq!(revision, 7),
            other => panic!("expected state.apply, got {other:?}"),
        }
    }

    /// The follow commands carry the id the lines come back under, so a
    /// Node with two consoles open can be told apart at the other end. A
    /// tag or a field lost in serialisation would silently route every line
    /// to the wrong console, or to none.
    #[test]
    fn the_follow_commands_round_trip_with_their_ids() {
        let follow_id = Uuid::new_v4();
        let command = DesktopCommand::FollowLogs { follow_id, container: "vibessh-app-1".to_string(), tail: 200 };
        let json = serde_json::to_value(&command).unwrap();
        assert_eq!(json["type"], "logs.follow");
        assert_eq!(json["container"], "vibessh-app-1");
        assert_eq!(json["tail"], 200);

        match serde_json::from_value::<DesktopCommand>(json).unwrap() {
            DesktopCommand::FollowLogs { follow_id: back, .. } => assert_eq!(back, follow_id),
            other => panic!("expected logs.follow, got {other:?}"),
        }

        let stop = serde_json::to_value(DesktopCommand::StopFollowingLogs { follow_id }).unwrap();
        assert_eq!(stop["type"], "logs.stop");
        match serde_json::from_value::<DesktopCommand>(stop).unwrap() {
            DesktopCommand::StopFollowingLogs { follow_id: back } => assert_eq!(back, follow_id),
            other => panic!("expected logs.stop, got {other:?}"),
        }
    }

    /// An Agent built before follows existed sends a line with no id. It has
    /// to keep deserialising: a parse failure there takes the whole
    /// connection down over a line the Desktop would simply have dropped.
    #[test]
    fn a_log_line_without_a_follow_id_still_parses() {
        let json = serde_json::json!({ "source": "docker", "line": "starting", "timestamp": "2026-09-02T10:00:00Z" });
        let line: crate::LogLine = serde_json::from_value(json).unwrap();
        assert_eq!(line.follow_id, None);
    }
}
