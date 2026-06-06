use crate::db;
use crate::types::*;
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use std::path::Path;
use uuid::Uuid;

/// The main engine driving T-minus coordination.
pub struct Engine {
    conn: Connection,
}

impl Engine {
    /// Create a new engine with SQLite storage at the given path.
    pub fn new(db_path: &Path) -> Result<Self, TMinusError> {
        let conn = db::init_db(db_path)?;
        Ok(Engine { conn })
        }

    /// Create an in-memory engine (for testing).
    pub fn in_memory() -> Result<Self, TMinusError> {
        let conn = db::init_db(Path::new(":memory:"))?;
        Ok(Engine { conn })
    }

    // ── Events ──────────────────────────────────────────────

    /// Schedule a new event.
    pub fn schedule_event(
        &mut self,
        kind: EventKind,
        scheduled_at: DateTime<Utc>,
        t_minus: Duration,
        organizer: AgentId,
        attendees: Vec<AgentId>,
        quorum: usize,
        payload: serde_json::Value,
    ) -> Result<TMinusEvent, TMinusError> {
        let event = TMinusEvent {
            id: Uuid::new_v4(),
            kind,
            scheduled_at,
            t_minus,
            organizer: organizer.clone(),
            attendees: attendees.into_iter().map(|a| (a, ResponseStatus::Pending)).collect(),
            quorum,
            payload,
        };
        db::insert_event(&self.conn, &event)?;
        Ok(event)
    }

    /// Confirm an agent's attendance.
    pub fn confirm(&mut self, event_id: Uuid, agent_id: &AgentId) -> Result<TMinusEvent, TMinusError> {
        let mut events = db::load_events(&self.conn)?;
        let event = events.iter_mut().find(|e| e.id == event_id)
            .ok_or(TMinusError::EventNotFound(event_id))?;

        let attendee = event.attendees.iter_mut().find(|(a, _)| a == agent_id)
            .ok_or(TMinusError::NotAttendee(agent_id.clone(), event_id))?;

        attendee.1 = ResponseStatus::Confirmed;
        db::insert_event(&self.conn, event)?;
        Ok(event.clone())
    }

    /// Defer an agent's response with a requested delay.
    pub fn defer(&mut self, event_id: Uuid, agent_id: &AgentId, duration: Duration) -> Result<TMinusEvent, TMinusError> {
        let mut events = db::load_events(&self.conn)?;
        let event = events.iter_mut().find(|e| e.id == event_id)
            .ok_or(TMinusError::EventNotFound(event_id))?;

        let attendee = event.attendees.iter_mut().find(|(a, _)| a == agent_id)
            .ok_or(TMinusError::NotAttendee(agent_id.clone(), event_id))?;

        attendee.1 = ResponseStatus::Deferred(duration);
        db::insert_event(&self.conn, event)?;
        Ok(event.clone())
    }

    /// Mark all pending attendees of an event as missed.
    pub fn mark_missed(&mut self, event_id: Uuid) -> Result<TMinusEvent, TMinusError> {
        let mut events = db::load_events(&self.conn)?;
        let event = events.iter_mut().find(|e| e.id == event_id)
            .ok_or(TMinusError::EventNotFound(event_id))?;

        for (_, status) in event.attendees.iter_mut() {
            if matches!(status, ResponseStatus::Pending) {
                *status = ResponseStatus::Missed;
            }
        }
        db::insert_event(&self.conn, event)?;
        Ok(event.clone())
    }

    /// Get all events.
    pub fn list_events(&self) -> Result<Vec<TMinusEvent>, TMinusError> {
        db::load_events(&self.conn)
    }

    /// Get a specific event.
    pub fn get_event(&self, id: Uuid) -> Result<Option<TMinusEvent>, TMinusError> {
        let events = db::load_events(&self.conn)?;
        Ok(events.into_iter().find(|e| e.id == id))
    }

    /// Remove a fired/missed event.
    pub fn remove_event(&mut self, id: Uuid) -> Result<bool, TMinusError> {
        db::delete_event(&self.conn, id)
    }

    // ── Campaigns ───────────────────────────────────────────

    /// Create a new campaign.
    pub fn create_campaign(&mut self, name: String) -> Result<Campaign, TMinusError> {
        let campaign = Campaign::new(name);
        db::insert_campaign(&self.conn, &campaign)?;
        Ok(campaign)
    }

    /// Add an event to a campaign.
    pub fn campaign_add_event(&mut self, campaign_id: Uuid, event_id: Uuid) -> Result<Campaign, TMinusError> {
        let mut campaign = db::load_campaign(&self.conn, campaign_id)?
            .ok_or(TMinusError::CampaignNotFound(campaign_id))?;

        if !campaign.events.contains(&event_id) {
            campaign.events.push(event_id);
        }
        db::insert_campaign(&self.conn, &campaign)?;
        Ok(campaign)
    }

    /// Add a dependency edge between two events in a campaign.
    pub fn campaign_link(&mut self, campaign_id: Uuid, from: Uuid, to: Uuid) -> Result<Campaign, TMinusError> {
        let mut campaign = db::load_campaign(&self.conn, campaign_id)?
            .ok_or(TMinusError::CampaignNotFound(campaign_id))?;

        if !campaign.events.contains(&from) {
            return Err(TMinusError::InvalidInput(format!("event {from} not in campaign")));
        }
        if !campaign.events.contains(&to) {
            return Err(TMinusError::InvalidInput(format!("event {to} not in campaign")));
        }

        if !campaign.dependencies.contains(&(from, to)) {
            campaign.dependencies.push((from, to));
        }
        db::insert_campaign(&self.conn, &campaign)?;

        // Validate no cycle
        if campaign.execution_order().is_err() {
            // Revert
            campaign.dependencies.pop();
            db::insert_campaign(&self.conn, &campaign)?;
            return Err(TMinusError::DependencyCycle(vec![from, to]));
        }

        Ok(campaign)
    }

    /// Get campaign execution order (topological sort).
    pub fn campaign_execution_order(&self, campaign_id: Uuid) -> Result<Vec<Uuid>, TMinusError> {
        let campaign = db::load_campaign(&self.conn, campaign_id)?
            .ok_or(TMinusError::CampaignNotFound(campaign_id))?;
        campaign.execution_order().map_err(TMinusError::DependencyCycle)
    }

    /// List all campaigns.
    pub fn list_campaigns(&self) -> Result<Vec<Campaign>, TMinusError> {
        db::load_campaigns(&self.conn)
    }

    /// Get a specific campaign.
    pub fn get_campaign(&self, id: Uuid) -> Result<Option<Campaign>, TMinusError> {
        db::load_campaign(&self.conn, id)
    }

    // ── Tick ────────────────────────────────────────────────

    /// Process the current moment: fire events that reached quorum, mark missed, handle deferrals.
    pub fn tick(&mut self, now: DateTime<Utc>) -> Result<TickResult, TMinusError> {
        let mut events = db::load_events(&self.conn)?;
        let mut tick = TickResult {
            fired: Vec::new(),
            missed: Vec::new(),
        };

        for event in events.iter_mut() {
            if now >= event.fire_time() {
                if event.has_quorum() {
                    tick.fired.push(event.id);
                } else {
                    // Check if any deferred attendees can still make it
                    let any_deferred = event.attendees.iter()
                        .any(|(_, s)| matches!(s, ResponseStatus::Deferred(_)));

                    if !any_deferred {
                        // Mark all pending as missed
                        for (_, status) in event.attendees.iter_mut() {
                            if matches!(status, ResponseStatus::Pending) {
                                *status = ResponseStatus::Missed;
                            }
                        }
                        tick.missed.push(event.id);
                    }
                    // If there are deferred attendees, give them more time (don't mark missed yet)
                }
                db::insert_event(&self.conn, event)?;
            }
        }

        // Remove fired events
        for id in &tick.fired {
            db::delete_event(&self.conn, *id)?;
        }

        Ok(tick)
    }

    /// Process a deferral cascade: when an agent defers, push the event's t_minus forward.
    /// Returns the updated event if the max deferral is applied.
    pub fn apply_deferral_cascade(
        &mut self,
        event_id: Uuid,
    ) -> Result<Option<TMinusEvent>, TMinusError> {
        let mut events = db::load_events(&self.conn)?;
        let event = events.iter_mut().find(|e| e.id == event_id)
            .ok_or(TMinusError::EventNotFound(event_id))?;

        let max_deferred = event.attendees.iter()
            .filter_map(|(_, s)| match s {
                ResponseStatus::Deferred(d) => Some(*d),
                _ => None,
            })
            .max()
            .unwrap_or(Duration::zero());

        if max_deferred > Duration::zero() {
            // Extend the fire time by the max deferral
            event.t_minus = event.t_minus + max_deferred;
            // Reset deferred attendees to pending
            for (_, status) in event.attendees.iter_mut() {
                if matches!(status, ResponseStatus::Deferred(_)) {
                    *status = ResponseStatus::Pending;
                }
            }
            db::insert_event(&self.conn, event)?;
            Ok(Some(event.clone()))
        } else {
            Ok(None)
        }
    }
}
