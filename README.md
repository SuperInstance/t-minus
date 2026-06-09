# t-minus

**T-minus event coordination for multi-agent systems.**

A Rust library + CLI for scheduling countdown events where agents confirm readiness before an event fires. Think of it as mission control for AI agent teams — coordinating meetings, reviews, deployments, and multi-step campaigns with quorum requirements.

[![Crates.io](https://img.shields.io/crates/v/t-minus.svg)](https://crates.io/crates/t-minus)
[![docs.rs](https://docs.rs/t-minus/badge.svg)](https://docs.rs/t-minus)
[![CI](https://github.com/SuperInstance/t-minus/actions/workflows/ci.yml/badge.svg)](https://github.com/SuperInstance/t-minus/actions)

---

## Architecture

```
┌─────────────────────────────────────────┐
│              CLI (main.rs)              │
│   schedule │ confirm │ defer │ tick     │
│   status   │ campaign create/link/order │
└─────────────────────────────────────────┘
                   │
┌─────────────────────────────────────────┐
│            Engine (engine.rs)           │
│  ┌─────────────┐    ┌───────────────┐  │
│  │   Events    │    │   Campaigns   │  │
│  │ schedule    │    │ create        │  │
│  │ confirm     │    │ add_event     │  │
│  │ defer       │    │ link (DAG)    │  │
│  │ tick        │    │ execution_order│ │
│  └─────────────┘    └───────────────┘  │
│            │                            │
│     ┌──────┴──────┐                     │
│     │   SQLite    │                     │
│     │  (db.rs)    │                     │
│     └─────────────┘                     │
└─────────────────────────────────────────┘
```

`t-minus` separates **event coordination** from **persistence**. The `Engine` holds all business logic; `db.rs` handles SQLite serialization. This makes the engine testable with an in-memory database (`Engine::in_memory()`) while production uses a file-backed store.

### Key Design Decisions

- **Quorum-gated firing** — Events only fire when enough agents confirm. No fire-and-forget.
- **Deferral cascade** — Agents can defer, extending the countdown. Deferred agents reset to pending after the extension.
- **Campaign DAGs** — Multi-step campaigns use dependency graphs with topological sorting and cycle detection.
- **Tick-based processing** — A single `tick(now)` call fires ready events and marks missed ones, making it easy to integrate with cron or event loops.

---

## Quick Start

### Library

```rust
use t_minus::{Engine, AgentId, EventKind};
use chrono::{Duration, Utc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::new("coordination.db".as_ref())?;

    // Schedule a deployment review in 2 hours with 1-hour countdown
    let event = engine.schedule_event(
        EventKind::Deploy,
        Utc::now() + Duration::hours(2),
        Duration::hours(1),
        AgentId("coordinator".into()),
        vec![AgentId("builder".into()), AgentId("auditor".into())],
        2, // quorum: both must confirm
        serde_json::json!({"crate": "fleet-midi"}),
    )?;

    // Agents confirm readiness
    engine.confirm(event.id, &AgentId("builder".into()))?;
    engine.confirm(event.id, &AgentId("auditor".into()))?;

    // Process: fires events that reached quorum
    let result = engine.tick(Utc::now())?;
    for id in &result.fired {
        println!("🚀 Event {} fired!", id);
    }
    Ok(())
}
```

### CLI

```bash
# Install
cargo install --git https://github.com/SuperInstance/t-minus

# Schedule a review in 5 minutes, needs 2 confirmations
t-minus schedule review +5m --quorum 2 --attendees alice,bob,carol

# Agent confirms readiness
t-minus confirm <event-id> alice

# Check status with live countdowns
t-minus status

# Process fired/missed events
t-minus tick

# Create a deployment pipeline campaign
t-minus campaign create "release-v2"
t-minus campaign add <campaign-id> checkpoint +10m
t-minus campaign link <campaign-id> <checkpoint-id> <review-id>
t-minus campaign order <campaign-id>   # topological sort
```

---

## API Reference

| Type / Function | Signature | Description |
|-----------------|-----------|-------------|
| `Engine::new` | `fn new(db_path: &Path) -> Result<Self, TMinusError>` | Create engine with SQLite persistence |
| `Engine::in_memory` | `fn in_memory() -> Result<Self, TMinusError>` | Create engine with `:memory:` store (testing) |
| `Engine::schedule_event` | `fn schedule_event(&mut self, kind, scheduled_at, t_minus, organizer, attendees, quorum, payload) -> Result<TMinusEvent, TMinusError>` | Schedule a new countdown event |
| `Engine::confirm` | `fn confirm(&mut self, event_id: Uuid, agent_id: &AgentId) -> Result<TMinusEvent, TMinusError>` | Agent confirms attendance |
| `Engine::defer` | `fn defer(&mut self, event_id: Uuid, agent_id: &AgentId, duration: Duration) -> Result<TMinusEvent, TMinusError>` | Agent defers with a delay |
| `Engine::tick` | `fn tick(&mut self, now: DateTime<Utc>) -> Result<TickResult, TMinusError>` | Process fired/missed events |
| `Engine::create_campaign` | `fn create_campaign(&mut self, name: String) -> Result<Campaign, TMinusError>` | Create a named campaign |
| `Engine::campaign_link` | `fn campaign_link(&mut self, campaign_id, from, to) -> Result<Campaign, TMinusError>` | Add dependency edge (cycle-safe) |
| `Engine::campaign_execution_order` | `fn campaign_execution_order(&self, campaign_id: Uuid) -> Result<Vec<Uuid>, TMinusError>` | Topological sort of campaign events |
| `TMinusEvent::has_quorum` | `fn has_quorum(&self) -> bool` | Whether confirmations meet quorum |
| `TMinusEvent::is_fired` | `fn is_fired(&self, now: DateTime<Utc>) -> bool` | Fire time passed + quorum met |
| `TMinusEvent::is_missed` | `fn is_missed(&self, now: DateTime<Utc>) -> bool` | Fire time passed + quorum not met |
| `Campaign::execution_order` | `fn execution_order(&self) -> Result<Vec<Uuid>, Vec<Uuid>>` | Kahn's algorithm topological sort |

### Core Types

```rust
pub struct TMinusEvent {
    pub id: Uuid,
    pub kind: EventKind,           // Meeting | Checkpoint | Review | Deploy | Custom
    pub scheduled_at: DateTime<Utc>,
    pub t_minus: Duration,
    pub organizer: AgentId,
    pub attendees: Vec<(AgentId, ResponseStatus)>,
    pub quorum: usize,
    pub payload: serde_json::Value,
}

pub enum ResponseStatus { Pending, Confirmed, Deferred(Duration), Missed }
pub struct Campaign { pub id: Uuid, pub name: String, pub events: Vec<Uuid>, pub dependencies: Vec<(Uuid, Uuid)> }
pub struct TickResult { pub fired: Vec<Uuid>, pub missed: Vec<Uuid> }
```

---

## Event Lifecycle

```
Schedule → Pending → [Confirmed | Deferred | Missed]
                        ↓                        ↓
                   Quorum met?              Defer → extend T-minus
                   ↓       ↓                    ↓
                 Fire!   Missed              Re-pend
```

1. **Schedule** — Organizer creates event with attendees, quorum, and T-minus countdown.
2. **Pending** — Attendees receive invitation; all start as `Pending`.
3. **Confirm** — Attendee signals readiness. When `confirmed_count() >= quorum`, event is ready.
4. **Defer** — Attendee requests delay. `apply_deferral_cascade()` extends T-minus by max deferral.
5. **Tick** — At fire time, events with quorum are **fired** and removed; events without quorum are **missed**.

---

## Integration Notes

### With Agent Orchestrators

`t-minus` is designed to be the coordination layer beneath an agent orchestrator. The orchestrator calls `tick()` periodically (e.g., every 30 seconds via a background task) and reacts to `TickResult`:

```rust
// In your orchestrator's event loop
loop {
    let result = engine.tick(Utc::now())?;
    for id in &result.fired {
        let event = engine.get_event(*id)?; // will be None (already removed)
        // Use your own event log or hook into the payload
    }
    std::thread::sleep(Duration::seconds(30).to_std()?);
}
```

### With SQLite-Backed Dashboards

The database schema is simple and queryable:

```sql
-- List upcoming events with quorum status
SELECT id, kind, scheduled_at, quorum, attendees FROM events;
-- List campaigns
SELECT id, name, events, dependencies FROM campaigns;
```

You can point a lightweight dashboard directly at `tminus.db` (SQLite supports concurrent reads).

### With External Notification Systems

The `payload` field is a `serde_json::Value`. Use it to store webhook URLs, Slack channel IDs, or MQTT topics. When an event fires, your integration layer reads the payload and routes notifications:

```rust
let event = engine.schedule_event(
    EventKind::Deploy,
    scheduled_at, t_minus, organizer, attendees, quorum,
    serde_json::json!({"webhook": "https://hooks.slack.com/...", "channel": "#deploys"}),
)?;
```

### Campaigns as CI/CD Pipelines

A campaign's dependency graph maps directly to CI/CD stages:

```rust
let campaign = engine.create_campaign("deploy-pipeline".into())?;
// Stage 1: Unit tests → Stage 2: Integration tests → Stage 3: Deploy
engine.campaign_link(campaign.id, test_event, integration_event)?;
engine.campaign_link(campaign.id, integration_event, deploy_event)?;
```

The `execution_order()` gives the exact sequence. Cycles are rejected at link time.

---

## Testing

```bash
cargo test        # 28 integration tests
cargo test -- --nocapture  # verbose
cargo build       # CLI binary
cargo run -- schedule meeting +5m --quorum 2 --attendees alice,bob
```

---

## License

MIT
