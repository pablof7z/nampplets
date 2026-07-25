use super::*;

#[test]
fn catalog_coordinates_are_parsed_only_at_the_rust_boundary() {
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
    for invalid in [
        "",
        "35129:author",
        "15129:author:extra",
        "unknown:author:d-tag",
        " 35129:author:d-tag",
    ] {
        assert!(
            parse_catalog_coordinate(invalid).is_err(),
            "unexpectedly accepted {invalid:?}"
        );
    }
}
