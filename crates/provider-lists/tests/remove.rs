use serde_json::json;

mod support;

use nmp_native_provider_lists::*;
use support::*;

#[test]
fn removing_a_present_entry_drafts_the_list_without_it() {
    let (provider, source) = provider_with(vec![entry("a"), entry("b"), entry("c")]);
    let (_registry, _observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "remove",
        json!({"list": follows(), "items": [p(&pubkey("b"))]}),
    );
    assert!(result.take_write_proposal().is_some());

    assert_eq!(
        source.drafted(),
        vec![vec![entry("a"), entry("c")]],
        "removal preserves the order of everything it keeps"
    );
}

#[test]
fn a_durable_removal_reports_its_count_under_removed() {
    let (provider, _source) = provider_with(vec![entry("a"), entry("b")]);
    let (_registry, observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "remove",
        json!({"list": follows(), "items": [p(&pubkey("b"))]}),
    );
    let (_write, completion, _work) = result.take_write_proposal().unwrap().into_parts();
    completion
        .into_receipt_sink()
        .push_latest(receipt("delivered"))
        .unwrap();

    let pushed = drain(&observer);
    assert_eq!(pushed[0]["type"], "lists.remove.result");
    assert_eq!(pushed[0]["removed"], 1);
    assert!(
        pushed[0].get("added").is_none(),
        "a removal never reports an added count"
    );
}

#[test]
fn removing_an_absent_entry_writes_nothing() {
    let (provider, source) = provider_with(vec![entry("a")]);
    let (_registry, _observer) = opened_session(provider.clone());

    let answer = response(
        &provider,
        "remove",
        json!({"list": follows(), "items": [p(&pubkey("f"))]}),
    );

    assert_eq!(answer["ok"], true);
    assert_eq!(answer["removed"], 0);
    assert_eq!(answer["skipped"], 1);
    assert!(source.drafted().is_empty());
}

#[test]
fn removing_from_an_empty_list_writes_nothing() {
    let (provider, source) = provider_with(Vec::new());
    let (_registry, _observer) = opened_session(provider.clone());

    let answer = response(
        &provider,
        "remove",
        json!({"list": follows(), "items": [p(&pubkey("b"))]}),
    );

    assert_eq!(answer["removed"], 0);
    assert!(source.drafted().is_empty());
}

#[test]
fn a_mixed_removal_removes_only_what_is_there() {
    let (provider, source) = provider_with(vec![entry("a"), entry("b")]);
    let (_registry, observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "remove",
        json!({"list": follows(), "items": [p(&pubkey("b")), p(&pubkey("f"))]}),
    );
    let (_write, completion, _work) = result.take_write_proposal().unwrap().into_parts();
    completion
        .into_receipt_sink()
        .push_latest(receipt("delivered"))
        .unwrap();

    assert_eq!(source.drafted(), vec![vec![entry("a")]]);
    let pushed = drain(&observer);
    assert_eq!(pushed[0]["removed"], 1);
    assert_eq!(pushed[0]["skipped"], 1);
}

/// A removal that empties a list is still a real change and must be written.
#[test]
fn removing_the_last_entry_proposes_a_write_of_the_empty_list() {
    let (provider, source) = provider_with(vec![entry("a")]);
    let (_registry, _observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "remove",
        json!({"list": follows(), "items": [p(&pubkey("a"))]}),
    );

    assert!(result.take_write_proposal().is_some());
    assert_eq!(source.drafted(), vec![Vec::<ListEntry>::new()]);
}
