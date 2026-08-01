//! Parsing for catalog coordinates accepted at the native boundary.

use nmp::{NostrEntity, decode_nostr_entity};
use nmp_native_artifact::ManifestCoordinate;

pub(crate) fn parse_catalog_coordinate(value: &str) -> Result<ManifestCoordinate, String> {
    if value.is_empty()
        || value.len() > 2_048
        || value.chars().any(char::is_control)
        || value.trim() != value
    {
        return Err(
            "coordinate must be 1..=2048 UTF-8 bytes without controls or surrounding whitespace"
                .to_owned(),
        );
    }

    let bech32_coordinate = value
        .get(..6)
        .filter(|prefix| prefix.eq_ignore_ascii_case("nostr:"))
        .map_or(value, |_| &value[6..]);
    if bech32_coordinate
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("naddr1"))
    {
        return match decode_nostr_entity(bech32_coordinate) {
            Ok(NostrEntity::Coordinate {
                kind: 35_129,
                author,
                identifier,
                ..
            }) => {
                ManifestCoordinate::named(&author, &identifier).map_err(|error| error.to_string())
            }
            Ok(NostrEntity::Coordinate { kind, .. }) => Err(format!(
                "naddr kind {kind} is not a named NAP manifest; expected kind 35129"
            )),
            Ok(_) => Err("catalog coordinate must be a NAP manifest naddr".to_owned()),
            Err(error) => Err(format!("invalid naddr: {error}")),
        };
    }

    let mut fields = value.splitn(3, ':');
    let kind = fields.next().unwrap_or_default();
    let first = fields
        .next()
        .ok_or_else(|| "coordinate is missing its author or event identifier".to_owned())?;
    let second = fields.next();
    let coordinate = match (kind, second) {
        ("5129", Some(author)) => ManifestCoordinate::snapshot(first, author),
        ("15129", None) => ManifestCoordinate::root(first),
        ("35129", Some(d_tag)) => ManifestCoordinate::named(first, d_tag),
        ("5129", None) => {
            return Err("snapshot coordinate must be 5129:event-id:author".to_owned());
        }
        ("35129", None) => {
            return Err("named coordinate must be 35129:author:d-tag".to_owned());
        }
        _ => {
            return Err(
                "supported coordinates are a kind-35129 naddr, 5129:event-id:author, 15129:author, and 35129:author:d-tag"
                    .to_owned(),
            );
        }
    };
    coordinate.map_err(|error| error.to_string())
}
