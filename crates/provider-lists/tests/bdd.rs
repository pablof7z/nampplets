//! Cucumber scenario runner for `crates/provider-lists`.
//!
//! Scenarios live under `tests/features/*.feature` as Gherkin. Every step
//! below drives the same fixture as the `#[test]` suites in this directory
//! (see `tests/support/mod.rs`), so a scenario and a unit test exercise
//! identical registration, session-binding and dispatch paths.

mod support;

use std::sync::Arc;

use cucumber::{World, given, then, when};
use nmp_native_nap_bridge::ProviderRegistry;
use nmp_native_provider_lists::*;
use serde_json::{Value, json};
use support::*;

#[derive(Debug, Default, World)]
struct ListsWorld {
    provider: Option<Arc<ListsProvider>>,
    source: Option<Arc<FakeSource>>,
    registry: Option<ProviderRegistry>,
    observer: Option<ProviderPushObserver>,
    call: Option<ProviderCall>,
    /// The immediate `.result` envelope, for answers that need no write.
    answer: Option<Value>,
    proposed: bool,
}

impl ListsWorld {
    fn provider(&self) -> Arc<ListsProvider> {
        self.provider.clone().expect("a provider is open")
    }

    fn source(&self) -> Arc<FakeSource> {
        self.source.clone().expect("a list source is open")
    }

    /// The result the napplet actually received, from whichever path
    /// produced it: an immediate answer or a receipt-driven push.
    fn result(&mut self) -> Value {
        if let Some(answer) = &self.answer {
            return answer.clone();
        }
        let pushed = drain(self.observer.as_ref().expect("a session is open"));
        assert_eq!(pushed.len(), 1, "expected exactly one result envelope");
        self.answer = Some(pushed[0].clone());
        pushed[0].clone()
    }

    fn mutate(&mut self, action: &str, list: Value, who: &str) {
        let provider = self.provider();
        let call = call(
            &provider,
            action,
            json!({"list": list, "items": [p(&named_key(who))]}),
        );
        self.proposed = call.write_proposal().is_some();
        self.answer = call
            .response
            .as_ref()
            .map(|response| response.decode().unwrap());
        self.call = Some(call);
    }

    fn take_completion(&mut self) -> Box<dyn nmp_native_nap_bridge::ProviderWriteCompletion> {
        let proposal = self
            .call
            .as_mut()
            .expect("a call was made")
            .take_write_proposal()
            .expect("a write was proposed");
        let (_write, completion, _work) = proposal.into_parts();
        completion
    }
}

/// Names in a scenario read as people, not as 64 hex characters.
fn named_key(who: &str) -> String {
    match who {
        "alice" => "a".repeat(64),
        "bob" => "b".repeat(64),
        other => panic!("unknown person {other:?} in a scenario"),
    }
}

#[given("a napplet with an open lists session")]
fn given_open_session(world: &mut ListsWorld) {
    let (provider, source) = provider_with(Vec::new());
    let (registry, observer) = opened_session(provider.clone());
    world.provider = Some(provider);
    world.source = Some(source);
    world.registry = Some(registry);
    world.observer = Some(observer);
}

#[given(regex = r#"^the account's follow list already contains "([^"]+)"$"#)]
fn given_follow_list_contains(world: &mut ListsWorld, who: String) {
    world
        .source()
        .set_entries(vec![ListEntry::new(ListItemTag::P, named_key(&who))]);
}

#[given("no account is connected")]
fn given_no_account(world: &mut ListsWorld) {
    world.source().sign_out();
}

#[when(regex = r#"^the napplet adds "([^"]+)" to the follow list$"#)]
fn when_add(world: &mut ListsWorld, who: String) {
    world.mutate("add", json!({"kind": 3}), &who);
}

#[when(regex = r#"^the napplet removes "([^"]+)" from the follow list$"#)]
fn when_remove(world: &mut ListsWorld, who: String) {
    world.mutate("remove", json!({"kind": 3}), &who);
}

#[when(regex = r#"^the napplet adds "([^"]+)" to list kind (\d+)$"#)]
fn when_add_to_kind(world: &mut ListsWorld, who: String, kind: u16) {
    world.mutate("add", json!({"kind": kind}), &who);
}

#[when("the write becomes durable")]
fn when_write_durable(world: &mut ListsWorld) {
    let completion = world.take_completion();
    completion
        .into_receipt_sink()
        .push_latest(receipt("delivered"))
        .unwrap();
}

#[when("the write fails")]
fn when_write_fails(world: &mut ListsWorld) {
    let completion = world.take_completion();
    completion
        .into_receipt_sink()
        .push_latest(receipt("failed"))
        .unwrap();
}

#[then("a write is proposed")]
fn then_write_proposed(world: &mut ListsWorld) {
    assert!(world.proposed, "expected a write proposal");
}

#[then("no write is proposed")]
fn then_no_write_proposed(world: &mut ListsWorld) {
    assert!(!world.proposed, "expected no write proposal");
    assert!(
        world.source().drafted().is_empty(),
        "a refused or no-op change must not draft a replacement"
    );
}

#[then(regex = r#"^the proposed list is exactly "([^"]*)"$"#)]
fn then_proposed_list(world: &mut ListsWorld, people: String) {
    let expected = people
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| ListEntry::new(ListItemTag::P, named_key(name)))
        .collect::<Vec<_>>();
    assert_eq!(world.source().drafted(), vec![expected]);
}

#[then("the napplet has not been told anything yet")]
fn then_nothing_told(world: &mut ListsWorld) {
    assert!(world.answer.is_none(), "the napplet was answered too early");
    assert!(
        drain(world.observer.as_ref().unwrap()).is_empty(),
        "the napplet was pushed a result before the write landed"
    );
}

#[then(regex = r"^the napplet received exactly (\d+) result$")]
fn then_result_count(world: &mut ListsWorld, expected: usize) {
    // One result is already consumed by `result()`; anything further is a
    // duplicate.
    let _ = world.result();
    assert_eq!(
        drain(world.observer.as_ref().unwrap()).len(),
        expected - 1,
        "the napplet received more than one result"
    );
}

#[then(regex = r"^the napplet is told (\d+) entr(?:y|ies) (?:was|were) added$")]
fn then_added(world: &mut ListsWorld, expected: usize) {
    assert_eq!(world.result()["added"], expected);
}

#[then(regex = r"^the napplet is told (\d+) entr(?:y|ies) (?:was|were) removed$")]
fn then_removed(world: &mut ListsWorld, expected: usize) {
    assert_eq!(world.result()["removed"], expected);
}

#[then(regex = r"^the napplet is told (\d+) entr(?:y|ies) (?:was|were) skipped$")]
fn then_skipped(world: &mut ListsWorld, expected: usize) {
    assert_eq!(world.result()["skipped"], expected);
}

#[then("the napplet is told the change did not succeed")]
fn then_not_ok(world: &mut ListsWorld) {
    let result = world.result();
    assert_eq!(result["ok"], false);
    assert!(result["error"].is_string(), "a refusal must say why");
}

#[then(regex = r#"^the napplet is told "([^"]+)"$"#)]
fn then_told_reason(world: &mut ListsWorld, reason: String) {
    let result = world.result();
    assert_eq!(result["ok"], false);
    assert_eq!(result["error"], reason);
}

#[then("the list was never read")]
fn then_never_read(world: &mut ListsWorld) {
    assert_eq!(world.source().reads(), 0);
}

// Single-threaded on purpose: the scenarios are deterministic and this crate
// does not pull in tokio's multi-thread runtime.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    ListsWorld::run("tests/features").await;
}
