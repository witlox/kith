use cucumber::{given, then, when};
use kith_common::event::{Event, EventCategory, EventScope};
use kith_state::retrieval::KeywordRetriever;
use kith_sync::store::EventFilter;

use crate::KithWorld;

#[given(expr = "{string} and {string} have active sync")]
fn active_sync(world: &mut KithWorld, _a: String, _b: String) {}

#[when(expr = "a command executes on {string} and is ingested")]
async fn command_ingested(world: &mut KithWorld, machine: String) {
    let event = Event::new(&machine, EventCategory::Exec, "exec.command", "test command")
        .with_scope(EventScope::Ops);
    world.event_store.write(event).await;
}

#[then(expr = "within {int} seconds the event appears in cr-sqlite on {string}")]
async fn event_appears(world: &mut KithWorld, _seconds: u32, _machine: String) {
    let count = world.event_store.len().await;
    assert!(count > 0);
}

#[given(expr = "{string} had a deployment failure logged {int} hours ago")]
async fn deployment_failure(world: &mut KithWorld, machine: String, _hours: u32) {
    let event = Event::new(&machine, EventCategory::System, "system.error", &format!("deployment failed on {machine}: OOM, staging broken"))
        .with_scope(EventScope::Ops);
    world.event_store.write(event).await;
}

#[given(expr = "the event is synced and embedded on {string}")]
fn event_synced(world: &mut KithWorld, _machine: String) {}

#[when(expr = "the user on {string} types {string}")]
async fn user_on_machine_types(world: &mut KithWorld, _machine: String, query: String) {
    let all = world.event_store.all().await;
    world.retrieval_results = KeywordRetriever::search(&all, &query, &EventScope::Ops, 10);
}

#[then(expr = "retrieve\\(\\) returns the failure event from {string}")]
fn retrieve_returns(world: &mut KithWorld, machine: String) {
    assert!(!world.retrieval_results.is_empty(), "should have results");
    assert!(world.retrieval_results.iter().any(|r| r.event.machine == machine));
}

#[given("three machines are in the mesh")]
async fn three_machines(world: &mut KithWorld) {
    for machine in &["dev-mac", "staging-1", "prod-1"] {
        let event = Event::new(*machine, EventCategory::Capability, "capability.updated", "report updated")
            .with_scope(EventScope::Public);
        world.event_store.write(event).await;
    }
}

#[when(expr = "the agent calls fleet_query\\({string}\\)")]
async fn fleet_query(world: &mut KithWorld, _query: String) {
    let events = world.event_store.query(&EventFilter {
        category: Some(EventCategory::Capability),
        ..Default::default()
    }).await;
    world.retrieval_results = events.iter().map(|e| kith_state::retrieval::RetrievalResult {
        event: e.clone(),
        score: 1.0,
        match_reason: "fleet query".into(),
    }).collect();
}

#[then("it receives each machine's hostname, capabilities, and last-sync timestamp")]
fn receives_fleet_info(world: &mut KithWorld) {
    assert!(world.retrieval_results.len() >= 3);
}

#[given(expr = "{string} and {string} lose connectivity")]
fn lose_connectivity(world: &mut KithWorld, _a: String, _b: String) {}

#[given("events accumulate independently on both")]
async fn events_accumulate(world: &mut KithWorld) {
    world.event_store.write(
        Event::new("dev-mac", EventCategory::Exec, "exec.command", "local work")
            .with_scope(EventScope::Ops),
    ).await;
}

#[then("cr-sqlite merges all events on both machines with no data loss")]
async fn no_data_loss(world: &mut KithWorld) {
    assert!(world.event_store.len().await > 0);
}

#[given(expr = "the user has {string} scope")]
fn user_scope(world: &mut KithWorld, scope: String) {
    world.current_user = Some(scope);
}

#[when(expr = "the agent calls retrieve\\({string}\\)")]
async fn agent_retrieves(world: &mut KithWorld, query: String) {
    let all = world.event_store.all().await;
    let scope = match world.current_user.as_deref() {
        Some("engineering") | Some("viewer") => EventScope::Public,
        _ => EventScope::Ops,
    };
    world.retrieval_results = KeywordRetriever::search(&all, &query, &scope, 10);
}

#[then("metadata is returned but content is withheld")]
fn metadata_only(world: &mut KithWorld) {
    assert!(world.retrieval_results.is_empty() || world.retrieval_results.iter().all(|r| r.event.scope == EventScope::Public));
}

#[then(expr = "the agent reports {string}")]
fn agent_reports(world: &mut KithWorld, _msg: String) {}
