use crate::types::*;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection};
use std::path::Path;
use uuid::Uuid;

/// Initialize the database schema.
pub fn init_db(path: &Path) -> Result<Connection, TMinusError> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS events (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            scheduled_at TEXT NOT NULL,
            t_minus_secs INTEGER NOT NULL,
            organizer TEXT NOT NULL,
            quorum INTEGER NOT NULL,
            payload TEXT NOT NULL,
            attendees TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS campaigns (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            events TEXT NOT NULL,
            dependencies TEXT NOT NULL
        );",
    )?;
    Ok(conn)
}

/// Insert an event into the database.
pub fn insert_event(conn: &Connection, event: &TMinusEvent) -> Result<(), TMinusError> {
    let attendees_json = serde_json::to_string(&event.attendees)?;
    conn.execute(
        "INSERT OR REPLACE INTO events (id, kind, scheduled_at, t_minus_secs, organizer, quorum, payload, attendees)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            event.id.to_string(),
            event.kind.to_string(),
            event.scheduled_at.to_rfc3339(),
            event.t_minus.num_seconds(),
            event.organizer.to_string(),
            event.quorum,
            event.payload.to_string(),
            attendees_json,
        ],
    )?;
    Ok(())
}

/// Load all events from the database.
pub fn load_events(conn: &Connection) -> Result<Vec<TMinusEvent>, TMinusError> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, scheduled_at, t_minus_secs, organizer, quorum, payload, attendees FROM events"
    )?;
    let rows = stmt.query_map([], |row| {
        let id_str: String = row.get(0)?;
        let kind_str: String = row.get(1)?;
        let scheduled_str: String = row.get(2)?;
        let t_minus_secs: i64 = row.get(3)?;
        let organizer_str: String = row.get(4)?;
        let quorum: usize = row.get(5)?;
        let payload_str: String = row.get(6)?;
        let attendees_str: String = row.get(7)?;
        Ok((id_str, kind_str, scheduled_str, t_minus_secs, organizer_str, quorum, payload_str, attendees_str))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (id_str, kind_str, scheduled_str, t_minus_secs, organizer_str, quorum, payload_str, attendees_str) = row?;
        let id: Uuid = id_str.parse().map_err(|e| TMinusError::InvalidInput(format!("bad uuid: {e}")))?;
        let kind: EventKind = kind_str.parse().map_err(|e| TMinusError::InvalidInput(e))?;
        let scheduled_at: DateTime<Utc> = scheduled_str.parse().map_err(|e| TMinusError::InvalidInput(format!("bad datetime: {e}")))?;
        let t_minus = Duration::seconds(t_minus_secs);
        let organizer = AgentId(organizer_str);
        let payload: serde_json::Value = serde_json::from_str(&payload_str)?;
        let attendees: Vec<(AgentId, ResponseStatus)> = serde_json::from_str(&attendees_str)?;

        events.push(TMinusEvent {
            id, kind, scheduled_at, t_minus, organizer, attendees, quorum, payload,
        });
    }
    Ok(events)
}

/// Delete an event from the database.
pub fn delete_event(conn: &Connection, id: Uuid) -> Result<bool, TMinusError> {
    let rows = conn.execute("DELETE FROM events WHERE id = ?1", params![id.to_string()])?;
    Ok(rows > 0)
}

/// Insert a campaign into the database.
pub fn insert_campaign(conn: &Connection, campaign: &Campaign) -> Result<(), TMinusError> {
    let events_json = serde_json::to_string(&campaign.events)?;
    let deps_json = serde_json::to_string(&campaign.dependencies)?;
    conn.execute(
        "INSERT OR REPLACE INTO campaigns (id, name, events, dependencies) VALUES (?1, ?2, ?3, ?4)",
        params![campaign.id.to_string(), campaign.name, events_json, deps_json],
    )?;
    Ok(())
}

/// Load all campaigns from the database.
pub fn load_campaigns(conn: &Connection) -> Result<Vec<Campaign>, TMinusError> {
    let mut stmt = conn.prepare("SELECT id, name, events, dependencies FROM campaigns")?;
    let rows = stmt.query_map([], |row| {
        let id_str: String = row.get(0)?;
        let name: String = row.get(1)?;
        let events_str: String = row.get(2)?;
        let deps_str: String = row.get(3)?;
        Ok((id_str, name, events_str, deps_str))
    })?;

    let mut campaigns = Vec::new();
    for row in rows {
        let (id_str, name, events_str, deps_str) = row?;
        let id: Uuid = id_str.parse().map_err(|e| TMinusError::InvalidInput(format!("bad uuid: {e}")))?;
        let events: Vec<Uuid> = serde_json::from_str(&events_str)?;
        let dependencies: Vec<(Uuid, Uuid)> = serde_json::from_str(&deps_str)?;
        campaigns.push(Campaign { id, name, events, dependencies });
    }
    Ok(campaigns)
}

/// Load a single campaign by id.
pub fn load_campaign(conn: &Connection, id: Uuid) -> Result<Option<Campaign>, TMinusError> {
    let campaigns = load_campaigns(conn)?;
    Ok(campaigns.into_iter().find(|c| c.id == id))
}
