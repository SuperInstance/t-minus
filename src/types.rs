use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for an agent in the system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for AgentId {
    fn from(s: &str) -> Self {
        AgentId(s.to_string())
    }
}

/// The kind of event being coordinated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Meeting,
    Checkpoint,
    Review,
    Deploy,
    Custom(String),
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventKind::Meeting => write!(f, "meeting"),
            EventKind::Checkpoint => write!(f, "checkpoint"),
            EventKind::Review => write!(f, "review"),
            EventKind::Deploy => write!(f, "deploy"),
            EventKind::Custom(s) => write!(f, "custom:{}", s),
        }
    }
}

impl std::str::FromStr for EventKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "meeting" => Ok(EventKind::Meeting),
            "checkpoint" => Ok(EventKind::Checkpoint),
            "review" => Ok(EventKind::Review),
            "deploy" => Ok(EventKind::Deploy),
            other => Ok(EventKind::Custom(other.to_string())),
        }
    }
}

/// An agent's response to an event invitation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseStatus {
    Pending,
    Confirmed,
    Deferred(Duration),
    Missed,
}

impl fmt::Display for ResponseStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResponseStatus::Pending => write!(f, "pending"),
            ResponseStatus::Confirmed => write!(f, "confirmed"),
            ResponseStatus::Deferred(d) => write!(f, "deferred:{}s", d.num_seconds()),
            ResponseStatus::Missed => write!(f, "missed"),
        }
    }
}

/// A T-minus countdown event for agent coordination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TMinusEvent {
    pub id: Uuid,
    pub kind: EventKind,
    pub scheduled_at: DateTime<Utc>,
    pub t_minus: Duration,
    pub organizer: AgentId,
    pub attendees: Vec<(AgentId, ResponseStatus)>,
    pub quorum: usize,
    pub payload: serde_json::Value,
}

impl TMinusEvent {
    /// The effective fire time: scheduled_at minus t_minus.
    pub fn fire_time(&self) -> DateTime<Utc> {
        self.scheduled_at - self.t_minus
    }

    /// Whether the event has reached quorum (enough confirmations).
    pub fn has_quorum(&self) -> bool {
        self.confirmed_count() >= self.quorum
    }

    /// Count of confirmed attendees.
    pub fn confirmed_count(&self) -> usize {
        self.attendees
            .iter()
            .filter(|(_, s)| matches!(s, ResponseStatus::Confirmed))
            .count()
    }

    /// Count of pending attendees.
    pub fn pending_count(&self) -> usize {
        self.attendees
            .iter()
            .filter(|(_, s)| matches!(s, ResponseStatus::Pending))
            .count()
    }

    /// Whether the event has fired (fire time has passed and quorum met).
    pub fn is_fired(&self, now: DateTime<Utc>) -> bool {
        now >= self.fire_time() && self.has_quorum()
    }

    /// Whether the event has been missed (fire time passed without quorum).
    pub fn is_missed(&self, now: DateTime<Utc>) -> bool {
        now >= self.fire_time() && !self.has_quorum()
    }

    /// Time remaining until fire time.
    pub fn time_remaining(&self, now: DateTime<Utc>) -> Duration {
        self.fire_time() - now
    }
}

/// A campaign is a sequence of events with dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub id: Uuid,
    pub name: String,
    pub events: Vec<Uuid>,
    pub dependencies: Vec<(Uuid, Uuid)>,
}

impl Campaign {
    pub fn new(name: String) -> Self {
        Campaign {
            id: Uuid::new_v4(),
            name,
            events: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    /// Topological sort of events based on dependencies.
    /// Returns Ok(sorted) or Err(cycle) if a dependency cycle exists.
    pub fn execution_order(&self) -> Result<Vec<Uuid>, Vec<Uuid>> {
        let mut in_degree: std::collections::HashMap<Uuid, usize> = std::collections::HashMap::new();
        let mut adj: std::collections::HashMap<Uuid, Vec<Uuid>> = std::collections::HashMap::new();

        for id in &self.events {
            in_degree.insert(*id, 0);
            adj.insert(*id, Vec::new());
        }

        for (from, to) in &self.dependencies {
            adj.get_mut(from).unwrap().push(*to);
            *in_degree.entry(*to).or_insert(0) += 1;
        }

        let mut queue: std::collections::VecDeque<Uuid> = std::collections::VecDeque::new();
        for (id, &deg) in &in_degree {
            if deg == 0 {
                queue.push_back(*id);
            }
        }

        let mut sorted = Vec::new();
        while let Some(id) = queue.pop_front() {
            sorted.push(id);
            if let Some(neighbors) = adj.get(&id) {
                for &next in neighbors {
                    let deg = in_degree.get_mut(&next).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(next);
                    }
                }
            }
        }

        if sorted.len() == self.events.len() {
            Ok(sorted)
        } else {
            // Return events in the cycle (those not in sorted)
            let cycle: Vec<Uuid> = self.events.iter().filter(|e| !sorted.contains(e)).copied().collect();
            Err(cycle)
        }
    }
}

/// Result of a tick operation — events that were fired or missed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickResult {
    pub fired: Vec<Uuid>,
    pub missed: Vec<Uuid>,
}

/// Errors that can occur in the engine.
#[derive(Debug, thiserror::Error)]
pub enum TMinusError {
    #[error("event not found: {0}")]
    EventNotFound(Uuid),
    #[error("campaign not found: {0}")]
    CampaignNotFound(Uuid),
    #[error("agent {0} not an attendee of event {1}")]
    NotAttendee(AgentId, Uuid),
    #[error("dependency cycle detected: {0:?}")]
    DependencyCycle(Vec<Uuid>),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}
