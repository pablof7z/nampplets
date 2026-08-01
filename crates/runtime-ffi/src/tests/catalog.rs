use super::*;

#[test]
fn catalog_coordinates_are_parsed_only_at_the_rust_boundary() {
    use nostr::{ToBech32, nips::nip01::Coordinate};

    let author = "a".repeat(64);
    let event_id = "b".repeat(64);
    assert!(matches!(
        parse_catalog_coordinate(&format!("35129:{author}:good-morning")).unwrap(),
        ManifestCoordinate::Named { .. }
    ));
    assert!(matches!(
        parse_catalog_coordinate(&format!("15129:{author}")).unwrap(),
        ManifestCoordinate::Root { .. }
    ));
    assert!(matches!(
        parse_catalog_coordinate(&format!("5129:{event_id}:{author}")).unwrap(),
        ManifestCoordinate::Snapshot { .. }
    ));
    let naddr = format!("35129:{author}:good-morning")
        .parse::<Coordinate>()
        .unwrap()
        .to_bech32()
        .unwrap();
    assert!(matches!(
        parse_catalog_coordinate(&naddr).unwrap(),
        ManifestCoordinate::Named { author: parsed, d_tag }
            if parsed.as_str() == author && d_tag.as_ref() == "good-morning"
    ));
    assert!(matches!(
        parse_catalog_coordinate(&format!("nostr:{naddr}")).unwrap(),
        ManifestCoordinate::Named { .. }
    ));
    assert!(matches!(
        parse_catalog_coordinate(&format!("NOSTR:{naddr}")).unwrap(),
        ManifestCoordinate::Named { .. }
    ));
    assert!(matches!(
        parse_catalog_coordinate(&format!("NoStR:{naddr}")).unwrap(),
        ManifestCoordinate::Named { .. }
    ));
    let wrong_kind = format!("30023:{author}:article")
        .parse::<Coordinate>()
        .unwrap()
        .to_bech32()
        .unwrap();
    assert!(parse_catalog_coordinate(&wrong_kind).is_err());
    for invalid in [
        "",
        "35129:author",
        "15129:author:extra",
        "unknown:author:d-tag",
        "naddr1broken",
        " 35129:author:d-tag",
    ] {
        assert!(
            parse_catalog_coordinate(invalid).is_err(),
            "unexpectedly accepted {invalid:?}"
        );
    }
}
