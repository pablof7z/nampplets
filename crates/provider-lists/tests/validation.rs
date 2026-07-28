use serde_json::json;

mod support;

use nmp_native_provider_lists::*;
use support::*;

/// Every refusal here is a protocol answer (`ok: false` with a reason), not a
/// transport error, and none of them may touch the list source.
fn refusal(entries: Vec<ListEntry>, payload: serde_json::Value) -> (serde_json::Value, usize) {
    let (provider, source) = provider_with(entries);
    let (_registry, _observer) = opened_session(provider.clone());
    let answer = response(&provider, "add", payload);
    let reads = source.reads();
    (answer, reads)
}

#[test]
fn an_unsupported_kind_is_named_rather_than_attempted() {
    let (answer, reads) = refusal(
        Vec::new(),
        json!({"list": {"kind": 1}, "items": [p(&pubkey("b"))]}),
    );

    assert_eq!(answer["ok"], false);
    assert_eq!(answer["error"], "this runtime does not service list kind 1");
    assert_eq!(reads, 0, "an unserviceable list is never read");
}

#[test]
fn a_parameterized_list_without_an_identifier_is_refused() {
    let (answer, reads) = refusal(
        Vec::new(),
        json!({"list": {"kind": 30000}, "items": [p(&pubkey("b"))]}),
    );

    assert_eq!(answer["ok"], false);
    assert_eq!(
        answer["error"],
        "list kind 30000 is addressed by a d identifier, which is missing"
    );
    assert_eq!(reads, 0);
}

#[test]
fn a_non_parameterized_list_with_an_identifier_is_refused() {
    let (answer, _) = refusal(
        Vec::new(),
        json!({"list": {"kind": 3, "identifier": "friends"}, "items": [p(&pubkey("b"))]}),
    );

    assert_eq!(answer["error"], "list kind 3 takes no d identifier");
}

#[test]
fn a_parameterized_list_is_addressed_by_its_exact_identifier() {
    let (provider, source) = provider_with(Vec::new());
    let (_registry, _observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "add",
        json!({"list": {"kind": 30000, "identifier": "friends"}, "items": [p(&pubkey("b"))]}),
    );

    assert!(result.take_write_proposal().is_some());
    assert_eq!(
        source.read_selectors(),
        vec![ListSelector {
            kind: 30_000,
            identifier: Some(Arc::from("friends")),
        }]
    );
}

#[test]
fn an_item_type_the_list_does_not_hold_is_refused() {
    // A follow list holds public keys; an event id is not a follow.
    let (answer, reads) = refusal(
        Vec::new(),
        json!({"list": follows(), "items": [{"type": "e", "value": pubkey("b")}]}),
    );

    assert_eq!(answer["error"], "list kind 3 does not accept e items");
    assert_eq!(reads, 0);
}

#[test]
fn a_malformed_public_key_is_refused_before_it_reaches_a_relay() {
    for value in ["", "not-hex", &"A".repeat(64), &"a".repeat(63)] {
        let (answer, reads) = refusal(Vec::new(), json!({"list": follows(), "items": [p(value)]}));
        assert_eq!(
            answer["error"], "a p value must be 64 lowercase hex characters",
            "accepted {value:?}"
        );
        assert_eq!(reads, 0);
    }
}

#[test]
fn an_uppercase_key_is_refused_so_one_entry_cannot_appear_twice() {
    let mixed = format!("{}{}", "A".repeat(2), "a".repeat(62));
    let (answer, _) = refusal(Vec::new(), json!({"list": follows(), "items": [p(&mixed)]}));

    assert_eq!(answer["ok"], false);
}

#[test]
fn an_empty_item_list_is_refused_rather_than_treated_as_a_no_op() {
    let (answer, reads) = refusal(Vec::new(), json!({"list": follows(), "items": []}));

    assert_eq!(answer["ok"], false);
    assert!(
        answer["error"]
            .as_str()
            .unwrap()
            .starts_with("items must hold 1..=")
    );
    assert_eq!(reads, 0);
}

#[test]
fn more_items_than_the_bound_are_refused_whole() {
    let limits = ListsProviderLimits::default();
    let items = (0..=limits.maximum_request_items)
        .map(|index| p(&format!("{index:064x}")))
        .collect::<Vec<_>>();
    let (answer, reads) = refusal(Vec::new(), json!({"list": follows(), "items": items}));

    assert_eq!(answer["ok"], false);
    assert_eq!(reads, 0, "an oversized request is never partially applied");
}

#[test]
fn the_same_item_twice_in_one_request_is_refused() {
    let (answer, reads) = refusal(
        Vec::new(),
        json!({"list": follows(), "items": [p(&pubkey("b")), p(&pubkey("b"))]}),
    );

    assert_eq!(
        answer["error"],
        "the same item appears more than once in one request"
    );
    assert_eq!(reads, 0);
}

#[test]
fn an_item_carrying_unknown_fields_is_refused() {
    let (answer, _) = refusal(
        Vec::new(),
        json!({"list": follows(), "items": [{"type": "p", "value": pubkey("b"), "relay": "wss://x"}]}),
    );

    assert_eq!(answer["ok"], false);
}

#[test]
fn a_selector_carrying_unknown_fields_is_refused() {
    let (answer, _) = refusal(
        Vec::new(),
        json!({"list": {"kind": 3, "author": pubkey("b")}, "items": [p(&pubkey("b"))]}),
    );

    assert_eq!(
        answer["error"],
        "list must be an object with a numeric kind"
    );
}

#[test]
fn a_change_that_would_cross_the_entry_bound_is_refused_whole() {
    let limits = ListsProviderLimits::default();
    let current = (0..limits.maximum_list_entries)
        .map(|index| ListEntry::new(ListItemTag::P, format!("{index:064x}")))
        .collect::<Vec<_>>();
    let (provider, source) = provider_with(current);
    let (_registry, _observer) = opened_session(provider.clone());

    let answer = response(
        &provider,
        "add",
        json!({"list": follows(), "items": [p(&pubkey("f"))]}),
    );

    assert_eq!(answer["ok"], false);
    assert!(
        answer["error"]
            .as_str()
            .unwrap()
            .contains("would exceed its")
    );
    assert!(
        source.drafted().is_empty(),
        "a list at its bound is never partially grown"
    );
}

#[test]
fn a_hashtag_with_whitespace_or_a_leading_hash_is_refused() {
    for value in ["", "two words", "#nostr"] {
        let (answer, _) = refusal(
            Vec::new(),
            json!({"list": {"kind": 10015}, "items": [{"type": "t", "value": value}]}),
        );
        assert_eq!(answer["ok"], false, "accepted {value:?}");
    }
}

#[test]
fn a_well_formed_hashtag_is_accepted() {
    let (provider, source) = provider_with(Vec::new());
    let (_registry, _observer) = opened_session(provider.clone());

    let mut result = call(
        &provider,
        "add",
        json!({"list": {"kind": 10015}, "items": [{"type": "t", "value": "nostr"}]}),
    );

    assert!(result.take_write_proposal().is_some());
    assert_eq!(
        source.drafted(),
        vec![vec![ListEntry::new(ListItemTag::T, "nostr")]]
    );
}

#[test]
fn a_replaceable_address_must_be_kind_pubkey_identifier() {
    for value in ["30023", "30023:short:x", &format!("x:{}:slug", pubkey("b"))] {
        let (answer, _) = refusal(
            Vec::new(),
            json!({"list": {"kind": 10003}, "items": [{"type": "a", "value": value}]}),
        );
        assert_eq!(answer["ok"], false, "accepted {value:?}");
    }

    let (provider, _) = provider_with(Vec::new());
    let (_registry, _observer) = opened_session(provider.clone());
    let mut result = call(
        &provider,
        "add",
        json!({
            "list": {"kind": 10003},
            "items": [{"type": "a", "value": format!("30023:{}:slug", pubkey("b"))}],
        }),
    );
    assert!(result.take_write_proposal().is_some());
}

#[test]
fn a_missing_correlation_id_is_a_transport_fault() {
    let (provider, _) = provider_with(Vec::new());
    let (_registry, _observer) = opened_session(provider.clone());
    let mut call = request(
        "add",
        json!({"list": follows(), "items": [p(&pubkey("b"))]}),
    );
    call.correlation_id = None;

    assert!(matches!(
        provider.call(call).unwrap_err(),
        nmp_native_nap_bridge::ProviderError::InvalidPayload { .. }
    ));
}

#[test]
fn a_mutation_missing_its_items_field_is_a_transport_fault() {
    let (provider, _) = provider_with(Vec::new());
    let (_registry, _observer) = opened_session(provider.clone());

    assert!(matches!(
        provider
            .call(request("add", json!({"list": follows()})))
            .unwrap_err(),
        nmp_native_nap_bridge::ProviderError::InvalidPayload { .. }
    ));
}
