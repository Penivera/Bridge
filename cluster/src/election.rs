use std::collections::HashSet;

use crate::protocol::PeerNode;

/// Lifecycle role of a node in the Bully election protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElectionRole {
    /// Follower recognizes an external active leader.
    Follower,
    /// Candidate is actively seeking election (sent Election or awaiting coordinator acks).
    Candidate,
    /// Leader is the elected cluster coordinator with verified quorum.
    Leader,
}

/// Dynamic state of the leader election state machine.
#[derive(Debug, Clone)]
pub struct ElectionState {
    /// Currently acknowledged cluster leader, if any.
    pub current_leader: Option<PeerNode>,
    /// Monotonically increasing election term.
    pub term: u64,
    /// Role of the local node in the current term.
    pub role: ElectionRole,
    /// Set of peer node IDs that acknowledged this node's Coordinator declaration for the current term.
    pub acks: HashSet<String>,
    /// Whether a higher-ranked peer answered with `ElectionOk` during the current election cycle.
    pub got_election_ok: bool,
    /// Explicit quorum override, if specified (otherwise dynamic majority based on total_known_nodes).
    pub quorum_size: Option<usize>,
    /// Total known nodes participating in the fleet (local node + known peers).
    pub total_known_nodes: usize,
}

impl Default for ElectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl ElectionState {
    pub fn new() -> Self {
        Self {
            current_leader: None,
            term: 0,
            role: ElectionRole::Follower,
            acks: HashSet::new(),
            got_election_ok: false,
            quorum_size: None,
            total_known_nodes: 1,
        }
    }

    /// Computes the required quorum count.
    /// If `quorum_size` is explicitly set, uses that.
    /// Otherwise returns the strict majority of total known nodes: `(total_known_nodes / 2) + 1`.
    pub fn quorum_required(&self) -> usize {
        if let Some(explicit) = self.quorum_size {
            explicit
        } else {
            (self.total_known_nodes / 2) + 1
        }
    }

    /// Verifies if quorum is achieved (self vote = 1 + unique peer coordinator acks >= quorum_required).
    pub fn has_quorum(&self) -> bool {
        (1 + self.acks.len()) >= self.quorum_required()
    }
}
