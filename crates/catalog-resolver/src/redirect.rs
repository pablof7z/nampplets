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

    let location = location
        .filter(|location| !location.is_empty() && location.len() <= maximum_url_bytes)
        .ok_or(AcquisitionRefusal::Redirect)?;
    let target = current
        .join(location)
        .map_err(|_| AcquisitionRefusal::Redirect)?;
    match validate_candidate(target.as_str(), maximum_url_bytes) {
        Ok(target) => Ok(ResponseAction::Follow(target)),
        Err(AcquisitionRefusal::InvalidCandidate) => Err(AcquisitionRefusal::Redirect),
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
            Err(AcquisitionRefusal::Redirect)
        );
        assert_eq!(
            classify_response(
                &current(),
                current().as_str(),
                302,
                Some("http://[::1"),
                MAXIMUM_URL_BYTES,
            ),
            Err(AcquisitionRefusal::Redirect)
        );
        assert_eq!(
            classify_response(
                &current(),
                current().as_str(),
                302,
                Some("x".repeat(MAXIMUM_URL_BYTES + 1).as_str()),
                MAXIMUM_URL_BYTES,
            ),
            Err(AcquisitionRefusal::Redirect)
        );
    }
}
