//! Rust-owned exact-install eligibility for one frozen catalog review.
//!
//! Native never re-derives whether a review may install. The decision is made
//! here with the same `Principal` invariants `catalog_confirm_install`
//! enforces at confirmation time, so a rendered install affordance and the
//! refusal Rust would actually return cannot drift apart. A blocked review
//! also carries the Rust-owned refusal code and reason text, so native
//! renders copy instead of authoring its own.

use nmp_native_catalog_resolver::ArtifactReview;
use nmp_native_runtime_core::Principal;

use super::{
    projection::{
        catalog_coordinate_string, coordinate_identity, project_lookup_facts, review_capabilities,
    },
    types::{RuntimeCatalogFailure, RuntimeCatalogReview},
};

const UNNAMED_MANIFEST_CODE: &str = "named-build-required";
const UNNAMED_MANIFEST_DETAIL: &str =
    "Only named manifests can mint an exact-build runtime principal.";
const UNSUPPORTED_IDENTITY_CODE: &str = "unsupported-manifest-identity";

/// Rust's exact-install decision for one frozen review.
///
/// `can_install` is the decision itself. `blocker` is present exactly when the
/// decision is negative and states, in Rust's own words, why the runtime would
/// refuse to mint an exact-build principal for this manifest. Its `provenance`
/// stays empty: the review's own provenance already covers the lookup.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeCatalogInstallEligibility {
    pub can_install: bool,
    pub blocker: Option<RuntimeCatalogFailure>,
}

pub(super) fn project_review(token: &str, review: &ArtifactReview) -> RuntimeCatalogReview {
    let summary = review.summary();
    let (manifest_author, d_tag) = coordinate_identity(summary.coordinate());
    let aggregate_hash = summary.aggregate().as_str().to_owned();
    let install_eligibility =
        decide_install_eligibility(&manifest_author, d_tag.as_deref(), &aggregate_hash);
    RuntimeCatalogReview {
        token: token.to_owned(),
        event_id: summary.event_id().as_str().to_owned(),
        coordinate: catalog_coordinate_string(summary.coordinate()),
        manifest_author,
        d_tag,
        title: summary.title().map(str::to_owned),
        description: summary.description().map(str::to_owned),
        aggregate_hash,
        capabilities: review_capabilities(summary),
        blob_sources: summary.servers().map(str::to_owned).collect(),
        provenance: project_lookup_facts(summary.lookup_facts()),
        install_eligibility,
    }
}

fn decide_install_eligibility(
    manifest_author: &str,
    d_tag: Option<&str>,
    aggregate_hash: &str,
) -> RuntimeCatalogInstallEligibility {
    let Some(d_tag) = d_tag else {
        return blocked(UNNAMED_MANIFEST_CODE, UNNAMED_MANIFEST_DETAIL.to_owned());
    };
    match Principal::new(manifest_author, d_tag, aggregate_hash) {
        Ok(_) => RuntimeCatalogInstallEligibility {
            can_install: true,
            blocker: None,
        },
        Err(error) => blocked(UNSUPPORTED_IDENTITY_CODE, error.to_string()),
    }
}

fn blocked(code: &str, detail: String) -> RuntimeCatalogInstallEligibility {
    RuntimeCatalogInstallEligibility {
        can_install: false,
        blocker: Some(RuntimeCatalogFailure {
            code: code.to_owned(),
            detail,
            provenance: Vec::new(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTHOR: &str = "266815e0c9210dfa324c6cba3573b14bee49da4209a9456f9484e5106cd408a5";
    const AGGREGATE: &str = "eea534010867c1fa6c41c012ea237b0ad77ec428693172010b124cb7f2048ade";

    fn blocker_code(eligibility: &RuntimeCatalogInstallEligibility) -> String {
        eligibility
            .blocker
            .as_ref()
            .expect("a blocked review must state why")
            .code
            .clone()
    }

    #[test]
    fn named_manifest_with_valid_identity_can_install() {
        let eligibility = decide_install_eligibility(AUTHOR, Some("stl-preview"), AGGREGATE);
        assert!(eligibility.can_install);
        assert_eq!(eligibility.blocker, None);
    }

    #[test]
    fn unnamed_manifest_is_blocked_with_rust_owned_copy() {
        let eligibility = decide_install_eligibility(AUTHOR, None, AGGREGATE);
        assert!(!eligibility.can_install);
        let blocker = eligibility
            .blocker
            .expect("a blocked review must state why");
        assert_eq!(blocker.code, UNNAMED_MANIFEST_CODE);
        assert_eq!(blocker.detail, UNNAMED_MANIFEST_DETAIL);
        assert!(blocker.provenance.is_empty());
    }

    #[test]
    fn identity_rejected_by_principal_invariants_is_blocked() {
        // Every one of these is named, so a `d_tag != nil` mirror would have
        // wrongly offered an install the runtime refuses at confirmation.
        let empty = decide_install_eligibility(AUTHOR, Some(""), AGGREGATE);
        assert!(!empty.can_install);
        assert_eq!(blocker_code(&empty), UNSUPPORTED_IDENTITY_CODE);

        let long_d_tag = "d".repeat(257);
        let overlong = decide_install_eligibility(AUTHOR, Some(&long_d_tag), AGGREGATE);
        assert!(!overlong.can_install);
        assert_eq!(blocker_code(&overlong), UNSUPPORTED_IDENTITY_CODE);

        let malformed = decide_install_eligibility(AUTHOR, Some("stl-preview"), "not-a-digest");
        assert!(!malformed.can_install);
        assert_eq!(blocker_code(&malformed), UNSUPPORTED_IDENTITY_CODE);
    }
}
