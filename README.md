# t-minus

**T-minus event coordination for multi-agent systems.**

A Rust library + CLI for scheduling countdown events where agents confirm readiness before an event fires. Think of it as mission control for AI agent teams — coordinating meetings, reviews, deployments, and multi-step campaigns with quorum requirements.

## The T-minus Concept

In aerospace, "T-minus 5 minutes" means an event happens 5 minutes from now. In agent coordination, **T-minus** means:

1. An event is scheduled at a future time
2. The system counts down to the "fire time" (scheduled time minus the T-minus duration)
3. Agents subscribe and respond: **confirm** (ready), **defer** (need more time), or **missed** (no response)
4. When quorum is reached (enough confirmations), the event **fires**
5. If fire time passes without quorum, the event is **missed**

This is essential for multi-agent systems where you need:
- **Rolling deployments** — confirm all agents are healthy before switching traffic
- **Code reviews** — wait for N reviewers to approve before merging
- **Coordinated campaigns** — sequential events where B depends on A completing
- **Meeting coordination** — gather quorum before starting

## Installation

```bash
cargo install --git https://github.com/SuperInstance/t-minus
```

## CLI Usage

### Schedule an event

```bash
# Schedule a code review in 5 minutes, needs 2 confirmations
t-minus schedule review +5m --quorum 2 --attendees alice,bob,carol

# Schedule a deployment at a specific time
t-minus schedule deploy "2025-06-06T15:00:00Z" --quorum 3 --attendees ops-1,ops-2,ops-3
```

### Agent responses

```bash
# Agent confirms readiness
t-minus confirm <event-id> alice

# Agent requests a delay
t-minus defer <event-id> bob 5m
```

### Check status

```bash
# Show all pending events with live countdowns
t-minus status
```

Output:
```
T-4m32s meeting [a1b2c3d4-...] quorum:2/3 attendees:3
   alice confirmed
   bob confirmed
   carol pending
```

### Process events

```bash
# Fire events that reached quorum, mark missed events
t-minus tick
```

Output:
```
🚀 Event a1b2c3d4-... fired! (quorum reached)
❌ Event e5f6g7h8-... missed (quorum not reached)
```

### Campaigns (multi-step coordination)

A campaign is a sequence of events with dependencies — event B starts only after event A confirms.

```bash
# Create a deployment pipeline campaign
t-minus campaign create "release-v2"

# Add events to the campaign
t-minus campaign add <campaign-id> checkpoint +10m
t-minus campaign add <campaign-id> review +20m
t-minus campaign add <campaign-id> deploy +30m

# Link dependencies: checkpoint → review → deploy
t-minus campaign link <campaign-id> <checkpoint-id> <review-id>
t-minus campaign link <campaign-id> <review-id> <deploy-id>

# View execution order (topological sort)
t-minus campaign order <campaign-id>

# List all campaigns
t-minus campaign list
```

## Library Usage

```rust
use t_minus::{Engine, AgentId, EventKind};
use chrono::{Duration, Utc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::new("coordination.db".as_ref())?;

    // Schedule a meeting
    let event = engine.schedule_event(
        EventKind::Meeting,
        Utc::now() + Duration::minutes(10),  // scheduled time
        Duration::minutes(5),                  // T-minus countdown
        AgentId("coordinator".into()),
        vec![AgentId("agent-1".into()), AgentId("agent-2".into())],
        2,  // quorum: both must confirm
        serde_json::json!({"topic": "sprint planning"}),
    )?;

    // Agents confirm
    engine.confirm(event.id, &AgentId("agent-1".into()))?;
    engine.confirm(event.id, &AgentId("agent-2".into()))?;

    // Process: fires events that reached quorum
    let result = engine.tick(Utc::now())?;
    for id in &result.fired {
        println!("Event {} fired!", id);
    }

    Ok(())
}
```

## Core Types

| Type | Description |
|------|-------------|
| `TMinusEvent` | A countdown event with attendees, quorum, and payload |
| `EventKind` | Meeting, Checkpoint, Review, Deploy, or Custom(String) |
| `ResponseStatus` | Pending, Confirmed, Deferred(Duration), or Missed |
| `Campaign` | A sequence of events with dependency edges |
| `AgentId` | Unique agent identifier (newtype wrapper) |
| `TickResult` | Lists of fired and missed event IDs from processing |

## Event Lifecycle

```
Schedule → Pending → [Confirmed | Deferred | Missed]
                        ↓                        ↓
                   Quorum met?              Defer → extend T-minus
                   ↓       ↓                    ↓
                 Fire!   Missed              Re-pend
```

## Campaign Execution Order

Campaigns use **topological sorting** to determine execution order. Events with no dependencies run first. Cycles are detected and rejected.

```
  A ──→ B ──→ D
  │           ↑
  └─→ C ──────┘

Order: A → B, C (parallel) → D
```

## Storage

Events are persisted in SQLite via `rusqlite`. All state survives restarts — schedule events, restart your system, and they'll still be there when you run `t-minus status`.

## Development

```bash
# Build
cargo build

# Run all 28 tests
cargo test

# Run CLI locally
cargo run -- schedule meeting +5m --quorum 2 --attendees alice,bob
```

## Architecture

```
src/
├── lib.rs       # Re-exports
├── types.rs     # Core types (TMinusEvent, Campaign, enums)
├── db.rs        # SQLite persistence layer
├── engine.rs    # Business logic (schedule, confirm, defer, tick)
└── main.rs      # CLI with clap subcommands

tests/
└── integration.rs  # 28 integration tests
```

## License

MIT
