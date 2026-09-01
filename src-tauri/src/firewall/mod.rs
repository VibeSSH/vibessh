//! Firewall - Etap M2. Mirrors `runtime::ApplicationRuntime`'s and
//! `files::ApplicationFileProvider`'s own shape deliberately: one trait,
//! one real SSH-exec-based backend so far (`ufw`), one match-site
//! constructor (`provider_for`) - the established VibeSSH pattern for "one
//! capability, several possible host-side backends," not a new one invented
//! for this feature.
//!
//! **Scope, and why**: rules are derived live from `application_ports`
//! (every port with `external_port` set, across every Application on a
//! Node, plus that Node's own SSH port - see
//! `services::firewall_service::reconcile_node`) - no `firewall_rules`
//! table tracking what was applied last time. `ufw allow` is itself
//! idempotent (re-adding an existing rule is a safe no-op), which is what
//! makes "always re-derive and re-apply the full desired set" safe.
//!
//! **Removal is real, but conservative on purpose.** A rule for a port
//! that's since been un-published, or belonged to an Application that's
//! been deleted or migrated to another Node, no longer appears in
//! `desired_rules` - `reconcile_node` diffs that against
//! `FirewallProvider::vibessh_owned_rules` and revokes exactly the
//! difference. The whole design turns on one thing: `vibessh_owned_rules`
//! must never report a rule this code didn't create itself, even one that
//! happens to allow the exact same port - deleting the host admin's own
//! rule by mistake would be a real, host-affecting error, not a
//! recoverable one. `UfwProvider` gets this for free from a feature ufw
//! already had (every rule this module applies carries `comment 'vibessh'`,
//! read back losslessly through `ufw show added` - see that impl's own doc
//! comment), not a new tracking table of its own. A backend that can't
//! prove a rule's origin this reliably must report `vibessh_owned_rules`
//! as empty rather than guess.
//!
//! **Enabling ufw itself is a deliberately separate, explicit action, never
//! automatic.** A host with ufw installed but inactive (the common default)
//! stays inactive after a reconcile - rules just queue up. Turning
//! *enforcement* on is a real, host-wide behavior change (every port not
//! explicitly allowed becomes unreachable, including for services VibeSSH
//! doesn't know anything about) that a user must trigger knowingly, not a
//! side effect of publishing one Application port. See `UfwProvider::enable`
//! for the one place that flips it, and its own doc comment for why it
//! always applies the current rule set (SSH port included) first, as part
//! of the same call, not as a separate step a caller could accidentally
//! reorder.

// Source restrictions for published Docker ports, which `ufw` alone
// cannot enforce - Docker's own iptables rules are evaluated before ufw's
// chain ever sees the packet. See the module's own doc comment.
pub mod docker_user;
pub mod ufw;

use crate::errors::AppResult;
use crate::models::PortProtocol;
use crate::ssh::SshSession;

/// One desired "allow" rule - a port + protocol pair. `source` is
/// deliberately not part of this yet (every rule ufw applies today is a
/// plain "allow from anywhere" rule); scoping a rule to the private Vibe
/// Network (Etap M/future) is a later addition to this struct, not a
/// different one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallRule {
    pub port: u16,
    pub protocol: PortProtocol,
    /// `None` = allow from anywhere (every rule before Etap M4). `Some(cidr)`
    /// scopes the rule to one source range - the Vibe Network mesh CIDR for
    /// a "Vibe Network only" Application port, or the mesh port itself.
    pub source_cidr: Option<String>,
}

#[async_trait::async_trait]
pub trait FirewallProvider: Send + Sync {
    /// A human-readable name for this backend (`"ufw"`) - surfaced to the
    /// UI so "which firewall is this Node using" is never a guess.
    fn name(&self) -> &'static str;

    /// Adds every rule in `desired` that isn't already present - safe to
    /// call repeatedly, and safe to call whether or not the firewall is
    /// currently enabled (see this module's own doc comment on why
    /// enabling is a separate, explicit step).
    async fn apply_rules(&self, connection: &SshSession, desired: &[FirewallRule]) -> AppResult<()>;

    /// The rules currently active, as best as they can be read back - for
    /// a future "current firewall state" view. Not used by the automatic
    /// reconcile path (`apply_rules` is unconditionally additive and
    /// idempotent, so it never needs to read state first).
    async fn current_rules(&self, connection: &SshSession) -> AppResult<Vec<FirewallRule>>;

    /// Every rule this exact backend created and is still present - the
    /// trusted signal `services::firewall_service::reconcile_node` diffs
    /// against `desired` to know what it's safe to remove (a port that's
    /// been unpublished, an Application that's been deleted or migrated
    /// away). Distinct from `current_rules`: this only ever returns a rule
    /// this backend can prove it added itself (`UfwProvider` does this via
    /// the `comment 'vibessh'` every `apply_rules` call already writes,
    /// read back through `ufw show added` rather than `ufw status` - see
    /// that impl's own doc comment for why the two differ). A rule the
    /// host's own admin added by hand must never appear here, even if it
    /// happens to allow the exact same port - this module's own doc
    /// comment above is the reasoning this method exists to finally make
    /// safe.
    async fn vibessh_owned_rules(&self, connection: &SshSession) -> AppResult<Vec<FirewallRule>>;

    /// Removes exactly the given rules - each must be a rule `apply_rules`
    /// itself could have created (same port/protocol/source_cidr shape).
    /// Only ever called with rules `vibessh_owned_rules` itself reported,
    /// never on a caller's own say-so.
    async fn revoke_rules(&self, connection: &SshSession, obsolete: &[FirewallRule]) -> AppResult<()>;

    /// Whether the firewall is actively enforcing (`ufw status` says
    /// `active`) - `false` doesn't mean rules are missing, only that
    /// nothing is being blocked yet.
    async fn is_active(&self, connection: &SshSession) -> AppResult<bool>;

    /// Applies `desired` (always including the Node's own SSH port - see
    /// `services::firewall_service`'s own doc comment on why that's a hard
    /// invariant enforced by the caller, not this trait) and only then
    /// turns enforcement on. Must never enable before applying - see this
    /// module's own doc comment.
    async fn enable(&self, connection: &SshSession, desired: &[FirewallRule]) -> AppResult<()>;
}

/// Detects which backend (if any) this Node actually has, mirroring
/// `runtime::runtime_for`'s "one match site" shape. `None` (not an error)
/// when nothing supported is present - a Node without ufw simply doesn't
/// get firewall sync, the same "an honest gap, not a silent one" stance
/// `runtime::docker::DockerRuntime::validate` already takes for Docker
/// itself.
pub async fn provider_for(connection: &SshSession) -> AppResult<Option<Box<dyn FirewallProvider>>> {
    if ufw::UfwProvider::detect(connection).await? {
        return Ok(Some(Box::new(ufw::UfwProvider)));
    }
    Ok(None)
}
