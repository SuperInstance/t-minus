use clap::{Parser, Subcommand};
use chrono::{DateTime, Duration, Utc};
use std::path::PathBuf;
use t_minus::{AgentId, Engine, EventKind};

#[derive(Parser)]
#[command(name = "t-minus", about = "T-minus event coordination for multi-agent systems")]
struct Cli {
    /// Database path
    #[arg(long, default_value = "tminus.db")]
    db: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Schedule a new event
    Schedule {
        /// Event kind: meeting, checkpoint, review, deploy, or custom name
        kind: String,
        /// Scheduled time (ISO 8601 or relative like +5m, +1h)
        time: String,
        /// Minimum confirmations needed
        #[arg(long, default_value = "1")]
        quorum: usize,
        /// Comma-separated attendee agent IDs
        #[arg(long, value_delimiter = ',')]
        attendees: Vec<String>,
        /// JSON payload
        #[arg(long, default_value = "{}")]
        payload: String,
    },
    /// Confirm attendance
    Confirm {
        event_id: String,
        agent_id: String,
    },
    /// Defer with a requested delay
    Defer {
        event_id: String,
        agent_id: String,
        /// Duration (e.g., 60s, 5m, 1h)
        duration: String,
    },
    /// Show all pending events with countdowns
    Status,
    /// Process current time, fire events, notify missed
    Tick,
    /// Campaign management
    Campaign {
        #[command(subcommand)]
        action: CampaignCommands,
    },
}

#[derive(Subcommand)]
enum CampaignCommands {
    /// Create a new campaign
    Create {
        name: String,
    },
    /// Add an event to a campaign
    Add {
        campaign_id: String,
        kind: String,
        time: String,
    },
    /// Link two events (dependency)
    Link {
        campaign_id: String,
        from_event: String,
        to_event: String,
    },
    /// Show campaign execution order
    Order {
        campaign_id: String,
    },
    /// List all campaigns
    List,
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.ends_with('s') {
        let secs: i64 = s.trim_end_matches('s').parse().map_err(|_| format!("invalid seconds: {s}"))?;
        Ok(Duration::seconds(secs))
    } else if s.ends_with('m') {
        let mins: i64 = s.trim_end_matches('m').parse().map_err(|_| format!("invalid minutes: {s}"))?;
        Ok(Duration::minutes(mins))
    } else if s.ends_with('h') {
        let hrs: i64 = s.trim_end_matches('h').parse().map_err(|_| format!("invalid hours: {s}"))?;
        Ok(Duration::hours(hrs))
    } else if s.ends_with('d') {
        let days: i64 = s.trim_end_matches('d').parse().map_err(|_| format!("invalid days: {s}"))?;
        Ok(Duration::days(days))
    } else {
        // Try as seconds
        let secs: i64 = s.parse().map_err(|_| format!("invalid duration: {s} (use 60s, 5m, 1h)"))?;
        Ok(Duration::seconds(secs))
    }
}

fn parse_time(s: &str) -> Result<DateTime<Utc>, String> {
    if s.starts_with('+') {
        let dur = parse_duration(&s[1..])?;
        Ok(Utc::now() + dur)
    } else {
        s.parse::<DateTime<Utc>>().map_err(|e| format!("invalid time '{s}': {e}"))
    }
}

fn parse_uuid(s: &str) -> Result<uuid::Uuid, String> {
    s.parse().map_err(|e| format!("invalid UUID '{s}': {e}"))
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::new(&cli.db)?;

    match cli.command {
        Commands::Schedule { kind, time, quorum, attendees, payload } => {
                        let kind = kind.parse::<EventKind>().map_err(|e: String| Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)) as Box<dyn std::error::Error>)?;
            let scheduled_at = parse_time(&time)?;
            let t_minus = Duration::zero(); // T-minus is the countdown; event fires at scheduled_at - t_minus
            let attendees: Vec<AgentId> = attendees.into_iter().map(AgentId).collect();
            let payload_val: serde_json::Value = serde_json::from_str(&payload)?;

            let event = engine.schedule_event(kind, scheduled_at, t_minus, AgentId("cli".into()), attendees, quorum, payload_val)?;
            println!("✅ Scheduled event {}", event.id);
            println!("   Kind: {}, Fire at: {}", event.kind, event.fire_time().to_rfc3339());
            println!("   Quorum: {}/{}", 0, event.quorum);
        }
        Commands::Confirm { event_id, agent_id } => {
            let eid = parse_uuid(&event_id)?;
            let agent = AgentId(agent_id);
            let event = engine.confirm(eid, &agent)?;
            println!("✅ {} confirmed for event {}", agent, event.id);
            println!("   Quorum: {}/{}", event.confirmed_count(), event.quorum);
        }
        Commands::Defer { event_id, agent_id, duration } => {
            let eid = parse_uuid(&event_id)?;
            let agent = AgentId(agent_id);
            let dur = parse_duration(&duration)?;
            let event = engine.defer(eid, &agent, dur)?;
            println!("⏳ {} deferred by {} for event {}", agent, duration, event.id);
        }
        Commands::Status => {
            let events = engine.list_events()?;
            let now = Utc::now();
            if events.is_empty() {
                println!("No pending events.");
            }
            for event in events {
                let remaining = event.time_remaining(now);
                let status = if remaining <= Duration::zero() {
                    "READY".to_string()
                } else {
                    format!("T-{}m{}s", remaining.num_minutes(), (remaining - Duration::minutes(remaining.num_minutes())).num_seconds())
                };
                let quorum_bar = format!("{}/{}", event.confirmed_count(), event.quorum);
                println!("{} {} [{}] quorum:{} attendees:{}",
                    status,
                    event.kind,
                    event.id,
                    quorum_bar,
                    event.attendees.len(),
                );
                for (agent, resp) in &event.attendees {
                    println!("   {} {}", agent, resp);
                }
            }
        }
        Commands::Tick => {
            let now = Utc::now();
            let result = engine.tick(now)?;
            for id in &result.fired {
                println!("🚀 Event {} fired! (quorum reached)", id);
            }
            for id in &result.missed {
                println!("❌ Event {} missed (quorum not reached)", id);
            }
            if result.fired.is_empty() && result.missed.is_empty() {
                println!("Nothing to process.");
            }
        }
        Commands::Campaign { action } => match action {
            CampaignCommands::Create { name } => {
                let campaign = engine.create_campaign(name)?;
                println!("✅ Campaign '{}' created: {}", campaign.name, campaign.id);
            }
            CampaignCommands::Add { campaign_id, kind, time } => {
                let cid = parse_uuid(&campaign_id)?;
                                let event_kind = kind.parse::<EventKind>().map_err(|e: String| Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)) as Box<dyn std::error::Error>)?;
                let scheduled_at = parse_time(&time)?;
                let event = engine.schedule_event(event_kind, scheduled_at, Duration::zero(), AgentId("cli".into()), vec![], 1, serde_json::Value::Null)?;
                engine.campaign_add_event(cid, event.id)?;
                println!("✅ Event {} added to campaign {}", event.id, cid);
            }
            CampaignCommands::Link { campaign_id, from_event, to_event } => {
                let cid = parse_uuid(&campaign_id)?;
                let from = parse_uuid(&from_event)?;
                let to = parse_uuid(&to_event)?;
                engine.campaign_link(cid, from, to)?;
                println!("✅ Linked {} → {} in campaign {}", from_event, to_event, campaign_id);
            }
            CampaignCommands::Order { campaign_id } => {
                let cid = parse_uuid(&campaign_id)?;
                let order = engine.campaign_execution_order(cid)?;
                println!("Execution order:");
                for (i, id) in order.iter().enumerate() {
                    println!("  {}. {}", i + 1, id);
                }
            }
            CampaignCommands::List => {
                let campaigns = engine.list_campaigns()?;
                if campaigns.is_empty() {
                    println!("No campaigns.");
                }
                for c in campaigns {
                    println!("{}: {} ({} events, {} deps)",
                        c.id, c.name, c.events.len(), c.dependencies.len());
                }
            }
        },
    }
    Ok(())
}
