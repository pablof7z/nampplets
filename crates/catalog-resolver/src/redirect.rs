use url::Url;

use crate::{AcquisitionRefusal, https::validate_candidate};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ResponseAction {
    Follow(Url),
    HandleStatus,
}

pub(super) fn classify_response(
    current: &Url,
    effective_url: &str,
    status: u16,
    location: Option<&str>,
    maximum_url_bytes: usize,
) -> Result<ResponseAction, AcquisitionRefusal> {
    if effective_url != current.as_str() {
        return Err(AcquisitionRefusal::SourceConfusion);
    }
    if !matches!(status, 301 | 302 | 303 | 307 | 308) {
        return Ok(ResponseAction::HandleStatus);
    }

    // Three distinct causes used to share one message ("is missing a valid
    // Location"), which made a Location header that was present but failed
    // to resolve read as if the header were absent. Report which of these
    // actually happened.
    let location =
        location
            .filter(|location| !location.is_empty())
            .ok_or(AcquisitionRefusal::Redirect {
                reason: "has no Location header",
            })?;
    if location.len() > maximum_url_bytes {
        return Err(AcquisitionRefusal::Redirect {
            reason: "has a Location header longer than the maximum candidate URL size",
        });
    }
    let target = current
        .join(location)
        .map_err(|_| AcquisitionRefusal::Redirect {
            reason: "has a Location header that could not be resolved against the current URL",
        })?;
    match validate_candidate(target.as_str(), maximum_url_bytes) {
        Ok(target) => Ok(ResponseAction::Follow(target)),
        Err(AcquisitionRefusal::InvalidCandidate) => Err(AcquisitionRefusal::Redirect {
            reason: "resolved to a Location that is not a valid HTTPS candidate",
        }),
        Err(reason) => Err(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAXIMUM_URL_BYTES: usize = 2_048;

    fn current() -> Url {
        Url::parse("https://source.example/blob").expect("valid test URL")
    }

    #[test]
    fn redirect_hop_source_confusion_precedes_following_location() {
        assert_eq!(
            classify_response(
                &current(),
                "https://other.example/blob",
                302,
                Some("https://next.example/blob"),
                MAXIMUM_URL_BYTES,
            ),
            Err(AcquisitionRefusal::SourceConfusion)
        );
    }

    #[test]
    fn successful_response_ignores_location() {
        assert_eq!(
            classify_response(
                &current(),
                current().as_str(),
                200,
                Some("https://next.example/blob"),
                MAXIMUM_URL_BYTES,
            ),
            Ok(ResponseAction::HandleStatus)
        );
    }

    #[test]
    fn not_modified_response_is_not_followed() {
        assert_eq!(
            classify_response(
                &current(),
                current().as_str(),
                304,
                Some("https://next.example/blob"),
                MAXIMUM_URL_BYTES,
            ),
            Ok(ResponseAction::HandleStatus)
        );
    }

    #[test]
    fn only_supported_redirect_statuses_with_valid_locations_are_followed() {
        for status in [301, 302, 303, 307, 308] {
            assert_eq!(
                classify_response(
                    &current(),
                    current().as_str(),
                    status,
                    Some("/next"),
                    MAXIMUM_URL_BYTES,
                ),
                Ok(ResponseAction::Follow(
                    Url::parse("https://source.example/next").expect("valid target")
                ))
            );
        }
        assert_eq!(
            classify_response(&current(), current().as_str(), 302, None, MAXIMUM_URL_BYTES,),
            Err(AcquisitionRefusal::Redirect {
                reason: "has no Location header"
            })
        );
        assert_eq!(
            classify_response(
                &current(),
                current().as_str(),
                302,
                Some("http://[::1"),
                MAXIMUM_URL_BYTES,
            ),
            Err(AcquisitionRefusal::Redirect {
                reason: "has a Location header that could not be resolved against the current URL"
            })
        );
        assert_eq!(
            classify_response(
                &current(),
                current().as_str(),
                302,
                Some("x".repeat(MAXIMUM_URL_BYTES + 1).as_str()),
                MAXIMUM_URL_BYTES,
            ),
            Err(AcquisitionRefusal::Redirect {
                reason: "has a Location header longer than the maximum candidate URL size"
            })
        );
    }

    #[test]
    fn redirect_refusal_causes_are_reported_distinctly_not_folded_into_one_message() {
        // Before this fix, an absent Location, an over-long Location, and a
        // Location that failed to resolve all produced the exact same
        // `AcquisitionRefusal::Redirect` value -- indistinguishable to
        // anything inspecting the refusal, including a Location that WAS
        // present but simply could not be joined against the current URL,
        // which read identically to "no Location header at all".
        let missing =
            classify_response(&current(), current().as_str(), 302, None, MAXIMUM_URL_BYTES);
        let too_long = classify_response(
            &current(),
            current().as_str(),
            302,
            Some("x".repeat(MAXIMUM_URL_BYTES + 1).as_str()),
            MAXIMUM_URL_BYTES,
        );
        let unresolvable = classify_response(
            &current(),
            current().as_str(),
            302,
            Some("http://[::1"),
            MAXIMUM_URL_BYTES,
        );
        assert_ne!(missing, too_long);
        assert_ne!(missing, unresolvable);
        assert_ne!(too_long, unresolvable);
    }
}
