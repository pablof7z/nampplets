//! The one place a relay lane is judged.
//!
//! Two lanes reach the runtime by different routes — operator relays shipped
//! in the host bundle, and profile relays a person edits at runtime — and both
//! must mean the same thing by "a usable relay". Encoding that twice is how a
//! second host ends up disagreeing with the first about which relays exist,
//! which is a difference nobody sees until routing quietly changes.

use nmp::RelayUrl;

/// Why one relay was not admitted.
///
/// The reason travels with the relay it refers to, so a lane can report what
/// it dropped rather than only that something was dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RelayRefusal {
    /// Anything other than `wss://`. Plaintext relay traffic is not a
    /// deployment choice this runtime offers.
    NotSecure,
    /// Credentials in the authority. A relay URL is not a place to put a
    /// secret, and one that arrives here has already been written down.
    CarriesCredentials,
    /// NMP itself could not parse it, so nothing downstream could route it.
    Unparseable,
    /// The same relay was already admitted to this lane.
    Duplicate,
    /// The lane had already reached its cap.
    LaneFull,
}

impl RelayRefusal {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::NotSecure => "must use a wss:// address",
            Self::CarriesCredentials => "must not carry credentials",
            Self::Unparseable => "is not a relay address NMP can parse",
            Self::Duplicate => "is already in this lane",
            Self::LaneFull => "did not fit inside this lane's limit",
        }
    }
}

/// Judges one relay in isolation. `admitted` carries the lane so far, which is
/// what makes a duplicate a duplicate.
pub(crate) fn judge_relay(relay: &str, admitted: &[String]) -> Result<(), RelayRefusal> {
    if !relay.starts_with("wss://") {
        return Err(RelayRefusal::NotSecure);
    }
    if relay.contains('@') {
        return Err(RelayRefusal::CarriesCredentials);
    }
    if RelayUrl::parse(relay).is_err() {
        return Err(RelayRefusal::Unparseable);
    }
    if admitted.iter().any(|existing| existing == relay) {
        return Err(RelayRefusal::Duplicate);
    }
    Ok(())
}

/// Refuses the whole lane on the first unusable relay, naming it.
///
/// This is the strict reading, used where a caller is editing the lane and can
/// fix what it just typed.
pub(crate) fn refuse_lane_on_first_fault(
    lane: &str,
    relays: &[String],
    maximum: usize,
    require_non_empty: bool,
) -> Result<(), String> {
    if require_non_empty && relays.is_empty() {
        return Err(format!(
            "{lane} relays must contain at least one secure relay"
        ));
    }
    if relays.len() > maximum {
        return Err(format!(
            "{lane} relays has {} entries; the maximum is {maximum}",
            relays.len()
        ));
    }
    let mut admitted: Vec<String> = Vec::with_capacity(relays.len());
    for relay in relays {
        // A duplicate is not a fault worth refusing an edit over: the lane the
        // caller meant is unambiguous. Every other refusal is.
        match judge_relay(relay, &admitted) {
            Ok(()) => admitted.push(relay.clone()),
            Err(RelayRefusal::Duplicate) => {}
            Err(_) => {
                return Err(format!(
                    "{lane} relays must use valid wss:// addresses without credentials"
                ));
            }
        }
    }
    Ok(())
}

/// One relay this runtime would not admit, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DroppedRelay {
    pub(crate) lane: &'static str,
    pub(crate) relay: String,
    pub(crate) refusal: RelayRefusal,
}

impl DroppedRelay {
    /// Reads as a sentence about the exact relay, so a deployment fault is
    /// legible without cross-referencing anything.
    pub(crate) fn detail(&self) -> String {
        format!(
            "{} relay {:?} {}",
            self.lane,
            self.relay,
            self.refusal.reason()
        )
    }
}

/// Admits the usable relays in a lane and names every one it drops.
///
/// Used for lanes that arrive as deployment inputs. Refusing such a lane
/// outright would turn one mistyped relay in a shipped bundle into a total
/// outage for everyone running that build, so the lane degrades instead — but
/// never silently: each drop leaves evidence, and an empty result is still the
/// caller's to refuse.
pub(crate) fn admit_lane(
    lane: &'static str,
    relays: &[String],
    maximum: usize,
) -> (Vec<String>, Vec<DroppedRelay>) {
    let mut admitted: Vec<String> = Vec::new();
    let mut dropped: Vec<DroppedRelay> = Vec::new();
    for relay in relays {
        if admitted.len() == maximum {
            // Past the cap the lane is full, and that is a property of the
            // lane rather than of this relay.
            dropped.push(DroppedRelay {
                lane,
                relay: relay.clone(),
                refusal: RelayRefusal::LaneFull,
            });
            continue;
        }
        match judge_relay(relay, &admitted) {
            Ok(()) => admitted.push(relay.clone()),
            Err(refusal) => dropped.push(DroppedRelay {
                lane,
                relay: relay.clone(),
                refusal,
            }),
        }
    }
    (admitted, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn judged(relay: &str) -> Result<(), RelayRefusal> {
        judge_relay(relay, &[])
    }

    #[test]
    fn a_secure_relay_is_admitted() {
        assert_eq!(judged("wss://relay.example"), Ok(()));
    }

    #[test]
    fn each_fault_is_named_separately() {
        assert_eq!(judged("ws://relay.example"), Err(RelayRefusal::NotSecure));
        assert_eq!(judged("http://relay.example"), Err(RelayRefusal::NotSecure));
        assert_eq!(
            judged("wss://user:pass@relay.example"),
            Err(RelayRefusal::CarriesCredentials)
        );
        assert_eq!(judged("wss://"), Err(RelayRefusal::Unparseable));
    }

    #[test]
    fn a_relay_already_in_the_lane_is_a_duplicate() {
        let admitted = vec!["wss://relay.example".to_owned()];
        assert_eq!(
            judge_relay("wss://relay.example", &admitted),
            Err(RelayRefusal::Duplicate)
        );
    }

    #[test]
    fn every_refusal_reads_as_a_sentence_about_the_relay() {
        for refusal in [
            RelayRefusal::NotSecure,
            RelayRefusal::CarriesCredentials,
            RelayRefusal::Unparseable,
            RelayRefusal::Duplicate,
            RelayRefusal::LaneFull,
        ] {
            assert!(!refusal.reason().is_empty());
        }
    }

    #[test]
    fn an_empty_required_lane_is_refused() {
        assert!(refuse_lane_on_first_fault("indexer", &[], 8, true).is_err());
        assert!(refuse_lane_on_first_fault("fallback", &[], 8, false).is_ok());
    }

    #[test]
    fn a_lane_past_its_cap_is_refused_whole() {
        let relays = (0..9)
            .map(|index| format!("wss://relay{index}.example"))
            .collect::<Vec<_>>();
        assert!(refuse_lane_on_first_fault("indexer", &relays, 8, true).is_err());
    }

    /// A repeated relay names the same lane the caller meant, so it is not
    /// worth refusing an edit over.
    #[test]
    fn a_duplicate_does_not_refuse_the_lane() {
        let relays = vec![
            "wss://relay.example".to_owned(),
            "wss://relay.example".to_owned(),
        ];
        assert_eq!(
            refuse_lane_on_first_fault("indexer", &relays, 8, true),
            Ok(())
        );
    }

    #[test]
    fn an_admitted_lane_keeps_the_usable_relays_in_order() {
        let relays = vec![
            "wss://a.example".to_owned(),
            "ws://b.example".to_owned(),
            "wss://c.example".to_owned(),
        ];
        let (admitted, dropped) = admit_lane("indexer", &relays, 8);

        assert_eq!(admitted, vec!["wss://a.example", "wss://c.example"]);
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].relay, "ws://b.example");
        assert_eq!(dropped[0].refusal, RelayRefusal::NotSecure);
    }

    /// The whole point of admitting rather than refusing: nothing disappears
    /// without saying so.
    #[test]
    fn every_dropped_relay_is_named_with_its_reason() {
        let relays = vec![
            "ws://plain.example".to_owned(),
            "wss://user:pass@creds.example".to_owned(),
            "wss://ok.example".to_owned(),
            "wss://ok.example".to_owned(),
        ];
        let (admitted, dropped) = admit_lane("app", &relays, 8);

        assert_eq!(admitted, vec!["wss://ok.example"]);
        assert_eq!(dropped.len(), 3);
        for drop in &dropped {
            assert!(drop.detail().contains(&drop.relay));
            assert!(drop.detail().starts_with("app relay "));
        }
    }

    #[test]
    fn relays_past_the_cap_are_reported_rather_than_forgotten() {
        let relays = (0..4)
            .map(|index| format!("wss://relay{index}.example"))
            .collect::<Vec<_>>();
        let (admitted, dropped) = admit_lane("indexer", &relays, 2);

        assert_eq!(admitted.len(), 2);
        assert_eq!(dropped.len(), 2);
        assert!(
            dropped
                .iter()
                .all(|drop| drop.refusal == RelayRefusal::LaneFull)
        );
    }

    #[test]
    fn an_entirely_unusable_lane_admits_nothing_and_says_so_four_times() {
        let relays = (0..4)
            .map(|i| format!("ws://plain{i}.example"))
            .collect::<Vec<_>>();
        let (admitted, dropped) = admit_lane("app", &relays, 8);

        assert!(admitted.is_empty());
        assert_eq!(dropped.len(), 4);
    }
}
