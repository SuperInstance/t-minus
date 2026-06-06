use chrono::{Duration, Utc};
use t_minus::*;
use uuid::Uuid;

fn make_engine() -> Engine {
    Engine::in_memory().unwrap()
}

fn agent(name: &str) -> AgentId {
    AgentId(name.into())
}

fn future_event(engine: &mut Engine, kind: EventKind, quorum: usize, attendees: Vec<&str>) -> TMinusEvent {
    engine.schedule_event(
        kind,
        Utc::now() + Duration::minutes(10),
        Duration::minutes(5),
        agent("organizer"),
        attendees.into_iter().map(|s| agent(s)).collect(),
        quorum,
        serde_json::json!({}),
    ).unwrap()
}

fn past_event(engine: &mut Engine, kind: EventKind, quorum: usize, attendees: Vec<&str>) -> TMinusEvent {
    engine.schedule_event(
        kind,
        Utc::now() - Duration::minutes(10),
        Duration::minutes(5),
        agent("organizer"),
        attendees.into_iter().map(|s| agent(s)).collect(),
        quorum,
        serde_json::json!({}),
    ).unwrap()
}

// ── Event creation ────────────────────────────────────────────

#[test]
fn test_event_creation() {
    let mut engine = make_engine();
    let event = future_event(&mut engine, EventKind::Meeting, 2, vec!["alice", "bob", "carol"]);
    assert_eq!(event.attendees.len(), 3);
    assert_eq!(event.quorum, 2);
    assert!(event.attendees.iter().all(|(_, s)| matches!(s, ResponseStatus::Pending)));
}

#[test]
fn test_event_kinds() {
    let mut engine = make_engine();
    let kinds = vec![EventKind::Meeting, EventKind::Checkpoint, EventKind::Review, EventKind::Deploy, EventKind::Custom("launch".into())];
    for kind in kinds {
        let event = future_event(&mut engine, kind.clone(), 1, vec!["a"]);
        assert_eq!(event.kind, kind);
    }
}

#[test]
fn test_event_fire_time() {
    let mut engine = make_engine();
    let scheduled = Utc::now() + Duration::minutes(10);
    let event = engine.schedule_event(
        EventKind::Meeting,
        scheduled,
        Duration::minutes(5),
        agent("org"),
        vec![agent("a")],
        1,
        serde_json::json!({}),
    ).unwrap();
    assert_eq!(event.fire_time(), scheduled - Duration::minutes(5));
}

// ── Confirmation ──────────────────────────────────────────────

#[test]
fn test_confirm_attendee() {
    let mut engine = make_engine();
    let event = future_event(&mut engine, EventKind::Meeting, 2, vec!["alice", "bob"]);
    let updated = engine.confirm(event.id, &agent("alice")).unwrap();
    assert_eq!(updated.confirmed_count(), 1);
    assert!(!updated.has_quorum());
}

#[test]
fn test_quorum_reached() {
    let mut engine = make_engine();
    let event = future_event(&mut engine, EventKind::Meeting, 2, vec!["alice", "bob"]);
    engine.confirm(event.id, &agent("alice")).unwrap();
    let updated = engine.confirm(event.id, &agent("bob")).unwrap();
    assert!(updated.has_quorum());
    assert_eq!(updated.confirmed_count(), 2);
}

#[test]
fn test_confirm_non_attendee_errors() {
    let mut engine = make_engine();
    let event = future_event(&mut engine, EventKind::Meeting, 1, vec!["alice"]);
    let result = engine.confirm(event.id, &agent("eve"));
    assert!(result.is_err());
}

#[test]
fn test_confirm_nonexistent_event() {
    let mut engine = make_engine();
    let result = engine.confirm(Uuid::new_v4(), &agent("alice"));
    assert!(result.is_err());
}

// ── Deferral ──────────────────────────────────────────────────

#[test]
fn test_defer_attendee() {
    let mut engine = make_engine();
    let event = future_event(&mut engine, EventKind::Review, 1, vec!["alice"]);
    let updated = engine.defer(event.id, &agent("alice"), Duration::minutes(5)).unwrap();
    assert!(matches!(updated.attendees[0].1, ResponseStatus::Deferred(_)));
}

#[test]
fn test_deferral_cascade_extends_t_minus() {
    let mut engine = make_engine();
    let event = past_event(&mut engine, EventKind::Deploy, 2, vec!["alice", "bob"]);
    let original_t_minus = event.t_minus;
    engine.defer(event.id, &agent("alice"), Duration::minutes(10)).unwrap();

    let result = engine.apply_deferral_cascade(event.id).unwrap().unwrap();
    assert!(result.t_minus > original_t_minus);
    // Deferred attendees reset to pending
    assert!(result.attendees.iter().all(|(_, s)| matches!(s, ResponseStatus::Pending)));
}

// ── Tick / Missed ─────────────────────────────────────────────

#[test]
fn test_tick_fires_quorum_event() {
    let mut engine = make_engine();
    let event = past_event(&mut engine, EventKind::Meeting, 1, vec!["alice"]);
    engine.confirm(event.id, &agent("alice")).unwrap();
    let result = engine.tick(Utc::now()).unwrap();
    assert!(result.fired.contains(&event.id));
    assert!(result.missed.is_empty());
}

#[test]
fn test_tick_marks_missed() {
    let mut engine = make_engine();
    let event = past_event(&mut engine, EventKind::Meeting, 2, vec!["alice", "bob"]);
    // No confirmations
    let result = engine.tick(Utc::now()).unwrap();
    assert!(result.missed.contains(&event.id));
}

#[test]
fn test_tick_with_deferred_gives_more_time() {
    let mut engine = make_engine();
    let event = past_event(&mut engine, EventKind::Review, 1, vec!["alice"]);
    engine.defer(event.id, &agent("alice"), Duration::minutes(5)).unwrap();
    let result = engine.tick(Utc::now()).unwrap();
    // Should not be missed or fired — deferred gives more time
    assert!(!result.fired.contains(&event.id));
    assert!(!result.missed.contains(&event.id));
}

#[test]
fn test_tick_fired_event_removed() {
    let mut engine = make_engine();
    let event = past_event(&mut engine, EventKind::Meeting, 1, vec!["alice"]);
    engine.confirm(event.id, &agent("alice")).unwrap();
    engine.tick(Utc::now()).unwrap();
    let loaded = engine.get_event(event.id).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn test_future_event_not_processed_by_tick() {
    let mut engine = make_engine();
    let event = future_event(&mut engine, EventKind::Meeting, 1, vec!["alice"]);
    engine.confirm(event.id, &agent("alice")).unwrap();
    let result = engine.tick(Utc::now()).unwrap();
    assert!(!result.fired.contains(&event.id));
    assert!(engine.get_event(event.id).unwrap().is_some());
}

// ── Missed detection ──────────────────────────────────────────

#[test]
fn test_mark_missed_pending() {
    let mut engine = make_engine();
    let event = past_event(&mut engine, EventKind::Meeting, 2, vec!["alice", "bob"]);
    engine.confirm(event.id, &agent("alice")).unwrap();
    engine.mark_missed(event.id).unwrap();
    let loaded = engine.get_event(event.id).unwrap().unwrap();
    assert_eq!(loaded.confirmed_count(), 1);
    let missed_count = loaded.attendees.iter().filter(|(_, s)| matches!(s, ResponseStatus::Missed)).count();
    assert_eq!(missed_count, 1);
}

// ── Persistence ───────────────────────────────────────────────

#[test]
fn test_events_persist_across_engine_instances() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let event_id = {
        let mut engine = Engine::new(&db_path).unwrap();
        let event = future_event(&mut engine, EventKind::Deploy, 1, vec!["alice"]);
        event.id
    };

    let engine2 = Engine::new(&db_path).unwrap();
    let loaded = engine2.get_event(event_id).unwrap().unwrap();
    assert_eq!(loaded.id, event_id);
}

// ── Campaign: topological sort ────────────────────────────────

#[test]
fn test_campaign_creation() {
    let mut engine = make_engine();
    let campaign = engine.create_campaign("release".into()).unwrap();
    assert_eq!(campaign.name, "release");
    assert!(campaign.events.is_empty());
}

#[test]
fn test_campaign_add_events() {
    let mut engine = make_engine();
    let campaign = engine.create_campaign("deploy".into()).unwrap();
    let e1 = future_event(&mut engine, EventKind::Checkpoint, 1, vec!["a"]);
    let e2 = future_event(&mut engine, EventKind::Deploy, 1, vec!["a"]);

    let updated = engine.campaign_add_event(campaign.id, e1.id).unwrap();
    assert_eq!(updated.events.len(), 1);
    let updated = engine.campaign_add_event(campaign.id, e2.id).unwrap();
    assert_eq!(updated.events.len(), 2);
}

#[test]
fn test_campaign_topological_sort_linear() {
    let mut engine = make_engine();
    let campaign = engine.create_campaign("pipeline".into()).unwrap();
    let e1 = future_event(&mut engine, EventKind::Checkpoint, 1, vec!["a"]);
    let e2 = future_event(&mut engine, EventKind::Review, 1, vec!["a"]);
    let e3 = future_event(&mut engine, EventKind::Deploy, 1, vec!["a"]);

    engine.campaign_add_event(campaign.id, e1.id).unwrap();
    engine.campaign_add_event(campaign.id, e2.id).unwrap();
    engine.campaign_add_event(campaign.id, e3.id).unwrap();
    engine.campaign_link(campaign.id, e1.id, e2.id).unwrap();
    engine.campaign_link(campaign.id, e2.id, e3.id).unwrap();

    let order = engine.campaign_execution_order(campaign.id).unwrap();
    assert_eq!(order.len(), 3);
    assert_eq!(order[0], e1.id);
    assert_eq!(order[1], e2.id);
    assert_eq!(order[2], e3.id);
}

#[test]
fn test_campaign_topological_sort_diamond() {
    let mut engine = make_engine();
    let campaign = engine.create_campaign("diamond".into()).unwrap();
    let a = future_event(&mut engine, EventKind::Custom("A".into()), 1, vec!["a"]);
    let b = future_event(&mut engine, EventKind::Custom("B".into()), 1, vec!["a"]);
    let c = future_event(&mut engine, EventKind::Custom("C".into()), 1, vec!["a"]);
    let d = future_event(&mut engine, EventKind::Custom("D".into()), 1, vec!["a"]);

    engine.campaign_add_event(campaign.id, a.id).unwrap();
    engine.campaign_add_event(campaign.id, b.id).unwrap();
    engine.campaign_add_event(campaign.id, c.id).unwrap();
    engine.campaign_add_event(campaign.id, d.id).unwrap();

    // A → B, A → C, B → D, C → D
    engine.campaign_link(campaign.id, a.id, b.id).unwrap();
    engine.campaign_link(campaign.id, a.id, c.id).unwrap();
    engine.campaign_link(campaign.id, b.id, d.id).unwrap();
    engine.campaign_link(campaign.id, c.id, d.id).unwrap();

    let order = engine.campaign_execution_order(campaign.id).unwrap();
    assert_eq!(order.len(), 4);
    let pos = |id: Uuid| order.iter().position(|&x| x == id).unwrap();
    assert!(pos(a.id) < pos(b.id));
    assert!(pos(a.id) < pos(c.id));
    assert!(pos(b.id) < pos(d.id));
    assert!(pos(c.id) < pos(d.id));
}

#[test]
fn test_campaign_cycle_detected() {
    let mut engine = make_engine();
    let campaign = engine.create_campaign("cycle".into()).unwrap();
    let a = future_event(&mut engine, EventKind::Custom("A".into()), 1, vec!["a"]);
    let b = future_event(&mut engine, EventKind::Custom("B".into()), 1, vec!["a"]);

    engine.campaign_add_event(campaign.id, a.id).unwrap();
    engine.campaign_add_event(campaign.id, b.id).unwrap();
    engine.campaign_link(campaign.id, a.id, b.id).unwrap();

    // This should fail — would create A→B→A cycle
    let result = engine.campaign_link(campaign.id, b.id, a.id);
    assert!(result.is_err());
}

#[test]
fn test_campaign_no_deps_any_order() {
    let mut engine = make_engine();
    let campaign = engine.create_campaign("parallel".into()).unwrap();
    let a = future_event(&mut engine, EventKind::Custom("A".into()), 1, vec!["a"]);
    let b = future_event(&mut engine, EventKind::Custom("B".into()), 1, vec!["a"]);
    let c = future_event(&mut engine, EventKind::Custom("C".into()), 1, vec!["a"]);

    engine.campaign_add_event(campaign.id, a.id).unwrap();
    engine.campaign_add_event(campaign.id, b.id).unwrap();
    engine.campaign_add_event(campaign.id, c.id).unwrap();

    let order = engine.campaign_execution_order(campaign.id).unwrap();
    assert_eq!(order.len(), 3);
    // All events present regardless of order
    assert!(order.contains(&a.id));
    assert!(order.contains(&b.id));
    assert!(order.contains(&c.id));
}

// ── Quorum edge cases ─────────────────────────────────────────

#[test]
fn test_quorum_zero_always_met() {
    let mut engine = make_engine();
    let event = past_event(&mut engine, EventKind::Meeting, 0, vec!["alice"]);
    assert!(event.has_quorum());
    let result = engine.tick(Utc::now()).unwrap();
    assert!(result.fired.contains(&event.id));
}

#[test]
fn test_quorum_equal_to_attendees() {
    let mut engine = make_engine();
    let event = past_event(&mut engine, EventKind::Meeting, 3, vec!["alice", "bob", "carol"]);
    engine.confirm(event.id, &agent("alice")).unwrap();
    engine.confirm(event.id, &agent("bob")).unwrap();
    assert!(!engine.get_event(event.id).unwrap().unwrap().has_quorum());
    engine.confirm(event.id, &agent("carol")).unwrap();
    assert!(engine.get_event(event.id).unwrap().unwrap().has_quorum());
}

// ── Event kind parsing ────────────────────────────────────────

#[test]
fn test_event_kind_from_str() {
    assert_eq!("meeting".parse::<EventKind>().unwrap(), EventKind::Meeting);
    assert_eq!("REVIEW".parse::<EventKind>().unwrap(), EventKind::Review);
    assert_eq!("custom_name".parse::<EventKind>().unwrap(), EventKind::Custom("custom_name".into()));
}

// ── Status display ────────────────────────────────────────────

#[test]
fn test_response_status_display() {
    assert_eq!(format!("{}", ResponseStatus::Pending), "pending");
    assert_eq!(format!("{}", ResponseStatus::Confirmed), "confirmed");
    assert_eq!(format!("{}", ResponseStatus::Missed), "missed");
    let deferred = ResponseStatus::Deferred(Duration::minutes(5));
    assert!(format!("{}", deferred).contains("deferred"));
}

// ── Campaign persistence ──────────────────────────────────────

#[test]
fn test_campaign_persists() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let cid = {
        let mut engine = Engine::new(&db_path).unwrap();
        let c = engine.create_campaign("persist-test".into()).unwrap();
        c.id
    };

    let engine2 = Engine::new(&db_path).unwrap();
    let loaded = engine2.get_campaign(cid).unwrap().unwrap();
    assert_eq!(loaded.name, "persist-test");
}

// ── Multiple ticks ────────────────────────────────────────────

#[test]
fn test_multiple_ticks_idempotent() {
    let mut engine = make_engine();
    let event = past_event(&mut engine, EventKind::Meeting, 1, vec!["alice"]);
    engine.confirm(event.id, &agent("alice")).unwrap();

    let r1 = engine.tick(Utc::now()).unwrap();
    assert!(r1.fired.contains(&event.id));

    // Second tick should not find the event (already removed)
    let r2 = engine.tick(Utc::now()).unwrap();
    assert!(!r2.fired.contains(&event.id));
}
