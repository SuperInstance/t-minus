//! Tutorial: t-minus — T-minus countdown event system for fleet coordination

use t_minus::{Engine, types::{AgentId, EventKind, TMinusEvent, ResponseStatus}};
use chrono::Utc;

fn main() {
    println!("=== T-Minus Tutorial ===\n");

    // Part 1: In-memory engine
    println!("Part 1: Engine setup");
    let mut engine = Engine::in_memory().unwrap();
    println!("  Engine created (in-memory)");
    println!();

    // Part 2: Schedule an event (7 args)
    println!("Part 2: Schedule event");
    let scheduled_at = Utc::now() + chrono::Duration::hours(2);
    let t_minus_dur = chrono::TimeDelta::hours(1); // 1-hour countdown
    let event = engine.schedule_event(
        EventKind::Deploy,
        scheduled_at,
        t_minus_dur,
        AgentId("commander".into()),
        vec![AgentId("builder".into()), AgentId("auditor".into())],
        2,
        serde_json::json!({"crate": "fleet-midi"}),
    ).unwrap();
    println!("  Event ID: {}", event.id);
    println!("  Fire time: {}", event.fire_time());
    println!("  Attendees: {}", event.attendees.len());
    println!("  Quorum needed: {}", event.quorum);
    println!();

    // Part 3: Confirm and check quorum
    println!("Part 3: Quorum confirmation");
    let event_id = event.id;
    let updated = engine.confirm(event_id, &AgentId("builder".into())).unwrap();
    let confirmed = updated.attendees.iter()
        .filter(|(_, s)| matches!(s, ResponseStatus::Confirmed))
        .count();
    println!("  Builder confirmed: {}/{}", confirmed, updated.quorum);
    println!("  Has quorum: {}", updated.has_quorum());
    println!();

    // Part 4: Defer
    println!("Part 4: Defer event");
    let deferred = engine.defer(event_id, &AgentId("auditor".into()), chrono::TimeDelta::seconds(300)).unwrap();
    println!("  Auditor deferred by 5 min");
    println!();

    // Part 5: List and inspect
    println!("Part 5: List all events");
    let events = engine.list_events().unwrap();
    println!("  {} events scheduled", events.len());
    for e in &events {
        println!("    ID: {} kind={:?}", e.id, e.kind);
    }
}
