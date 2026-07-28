use serde_json::json;

mod support;

use nmp_native_provider_lists::*;
use support::*;

/// The whole point of the domain: adding something new proposes exactly one
/// write, carrying the current list plus the addition and nothing else.
#[test]
fn adding_a_new_entry_proposes_a_write_of_the_exact_resulting_list() {
    let (provider, source) = provider_with(vec![entry("a")]);
    let (_registry, _observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "add",
        json!({"list": follows(), "items": [p(&pubkey("b"))]}),
    );

    // Nothing is told to the napplet yet — the list has not changed until NMP
    // says it has.
    assert!(result.response.is_none());
    let proposal = result.take_write_proposal().expect("a write is proposed");
    let (write, _completion, _work) = proposal.into_parts();
    assert_eq!(write.origin_principal, principal());
    assert_eq!(write.account.0.as_ref(), &pubkey("a"));

    assert_eq!(
        source.drafted(),
        vec![vec![entry("a"), entry("b")]],
        "the draft is the current list with the addition appended"
    );
    assert_eq!(
        source.read_selectors(),
        vec![ListSelector {
            kind: 3,
            identifier: None
        }]
    );
}

#[test]
fn the_napplet_result_is_emitted_only_once_the_write_is_durable() {
    let (provider, _source) = provider_with(vec![entry("a")]);
    let (_registry, observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "add",
        json!({"list": follows(), "items": [p(&pubkey("b")), p(&pubkey("c"))]}),
    );
    let (_write, completion, _work) = result.take_write_proposal().unwrap().into_parts();
    let sink = completion.into_receipt_sink();

    // In flight: still silent.
    sink.push_latest(receipt("pending")).unwrap();
    assert!(drain(&observer).is_empty());

    sink.push_latest(receipt("delivered")).unwrap();
    let pushed = drain(&observer);
    assert_eq!(pushed.len(), 1);
    assert_eq!(pushed[0]["type"], "lists.add.result");
    assert_eq!(pushed[0]["id"], "request-1");
    assert_eq!(pushed[0]["ok"], true);
    assert_eq!(pushed[0]["added"], 2);
    assert_eq!(pushed[0]["skipped"], 0);
}

#[test]
fn a_failed_write_reports_zero_added_rather_than_the_requested_count() {
    let (provider, _source) = provider_with(vec![entry("a")]);
    let (_registry, observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "add",
        json!({"list": follows(), "items": [p(&pubkey("b"))]}),
    );
    let (_write, completion, _work) = result.take_write_proposal().unwrap().into_parts();
    completion
        .into_receipt_sink()
        .push_latest(receipt("failed"))
        .unwrap();

    let pushed = drain(&observer);
    assert_eq!(pushed.len(), 1);
    assert_eq!(pushed[0]["ok"], false);
    assert_eq!(pushed[0]["added"], 0);
    assert!(pushed[0]["error"].is_string());
}

#[test]
fn a_refused_write_still_answers_the_napplet() {
    let (provider, _source) = provider_with(vec![entry("a")]);
    let (_registry, observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "add",
        json!({"list": follows(), "items": [p(&pubkey("b"))]}),
    );
    result
        .take_write_proposal()
        .unwrap()
        .refuse(Arc::from("the user declined"));

    let pushed = drain(&observer);
    assert_eq!(pushed.len(), 1);
    assert_eq!(pushed[0]["ok"], false);
    assert_eq!(pushed[0]["error"], "the user declined");
}

#[test]
fn only_one_result_is_ever_emitted_for_one_mutation() {
    let (provider, _source) = provider_with(vec![entry("a")]);
    let (_registry, observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "add",
        json!({"list": follows(), "items": [p(&pubkey("b"))]}),
    );
    let (_write, completion, _work) = result.take_write_proposal().unwrap().into_parts();
    let sink = completion.into_receipt_sink();
    sink.push_latest(receipt("delivered")).unwrap();
    sink.push_latest(receipt("failed")).unwrap();
    sink.close(Some(Arc::from("late close")));

    assert_eq!(drain(&observer).len(), 1);
}

/// A no-op must not burn a durable write or move the replaceable event's
/// timestamp.
#[test]
fn adding_an_entry_that_is_already_present_writes_nothing() {
    let (provider, source) = provider_with(vec![entry("a"), entry("b")]);
    let (_registry, _observer) = opened_session(provider.clone());

    let answer = response(
        &provider,
        "add",
        json!({"list": follows(), "items": [p(&pubkey("b"))]}),
    );

    assert_eq!(answer["ok"], true);
    assert_eq!(answer["added"], 0);
    assert_eq!(answer["skipped"], 1);
    assert!(source.drafted().is_empty(), "no draft, so no write");
}

#[test]
fn a_partially_new_request_counts_added_and_skipped_separately() {
    let (provider, source) = provider_with(vec![entry("a")]);
    let (_registry, _observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "add",
        json!({"list": follows(), "items": [p(&pubkey("a")), p(&pubkey("b"))]}),
    );
    let (_write, completion, _work) = result.take_write_proposal().unwrap().into_parts();
    let sink = completion.into_receipt_sink();
    sink.push_latest(receipt("delivered")).unwrap();

    assert_eq!(source.drafted(), vec![vec![entry("a"), entry("b")]]);
}

#[test]
fn a_list_the_account_has_never_published_is_created_by_the_first_change() {
    let (provider, source) = provider_with(Vec::new());
    let (_registry, _observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "add",
        json!({"list": follows(), "items": [p(&pubkey("b"))]}),
    );

    assert!(result.take_write_proposal().is_some());
    assert_eq!(source.drafted(), vec![vec![entry("b")]]);
}

#[test]
fn signing_out_refuses_the_change_instead_of_writing_under_another_key() {
    let (provider, source) = provider_with(vec![entry("a")]);
    let (_registry, _observer) = opened_session(provider.clone());
    source.sign_out();

    let answer = response(
        &provider,
        "add",
        json!({"list": follows(), "items": [p(&pubkey("b"))]}),
    );

    assert_eq!(answer["ok"], false);
    assert_eq!(answer["added"], 0);
    assert_eq!(
        answer["error"],
        "no account is connected, so there is no list to change"
    );
    assert_eq!(source.reads(), 0, "no list is read without an account");
}

#[test]
fn a_source_read_failure_is_a_transport_fault_not_a_silent_success() {
    let (provider, source) = provider_with(vec![entry("a")]);
    let (_registry, _observer) = opened_session(provider.clone());
    source.fail_reads(ListsDataError::Closed);

    let error = provider
        .call(request(
            "add",
            json!({"list": follows(), "items": [p(&pubkey("b"))]}),
        ))
        .unwrap_err();

    assert!(matches!(
        error,
        nmp_native_nap_bridge::ProviderError::Failed { .. }
    ));
}

#[test]
fn an_undraftable_change_is_refused_before_any_write_is_proposed() {
    let (provider, source) = provider_with(vec![entry("a")]);
    let (_registry, _observer) = opened_session(provider.clone());
    source.fail_drafts(ListsDataError::DraftTooLarge);

    let error = provider
        .call(request(
            "add",
            json!({"list": follows(), "items": [p(&pubkey("b"))]}),
        ))
        .unwrap_err();

    assert!(matches!(
        error,
        nmp_native_nap_bridge::ProviderError::Failed { .. }
    ));
}

#[test]
fn a_mutation_without_an_open_session_is_denied() {
    let (provider, source) = provider_with(vec![entry("a")]);

    let error = provider
        .call(request(
            "add",
            json!({"list": follows(), "items": [p(&pubkey("b"))]}),
        ))
        .unwrap_err();

    assert!(matches!(
        error,
        nmp_native_nap_bridge::ProviderError::Denied { .. }
    ));
    assert_eq!(source.reads(), 0);
}
