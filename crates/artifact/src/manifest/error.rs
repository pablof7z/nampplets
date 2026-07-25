use thiserror::Error;

use crate::ArtifactError;

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("manifest verifier limits must be finite and non-zero")]
    InvalidLimits,
    #[error("manifest event is {actual} bytes; the maximum is {maximum}")]
    EventTooLarge { actual: usize, maximum: usize },
    #[error("manifest event is not valid Nostr event JSON: {0}")]
    EventJson(#[source] serde_json::Error),
    #[error("manifest event id does not match its canonical NIP-01 serialization")]
    InvalidEventId,
    #[error("manifest event Schnorr signature is invalid")]
    InvalidEventSignature,
    #[error("event kind {0} is not a pinned NIP-5D manifest kind")]
    UnsupportedKind(u16),
    #[error(
        "resolved event kind differs from the requested coordinate: expected {expected}, got {actual}"
    )]
    CoordinateKind { expected: u16, actual: u16 },
    #[error("resolved event author differs from the requested coordinate")]
    CoordinateAuthor,
    #[error("resolved snapshot event id differs from the requested coordinate")]
    CoordinateEventId,
    #[error("resolved named-manifest d tag differs from the requested coordinate")]
    CoordinateDTag,
    #[error("named manifest is missing its d tag")]
    MissingDTag,
    #[error("root or snapshot manifest contains an unexpected d tag")]
    UnexpectedDTag,
    #[error("manifest d tag is empty, normalized, over-limit, or contains control characters")]
    InvalidDTag,
    #[error("manifest has {actual} tags; the maximum is {maximum}")]
    TagCount { actual: usize, maximum: usize },
    #[error("manifest tag {name:?} has {actual} fields; the maximum is {maximum}")]
    TagFieldCount {
        name: String,
        actual: usize,
        maximum: usize,
    },
    #[error("manifest tag {name:?} contains a {actual}-byte field; the maximum is {maximum}")]
    TagStringTooLarge {
        name: String,
        actual: usize,
        maximum: usize,
    },
    #[error("critical manifest tag {name:?} must have {expected} fields, not {actual}")]
    MalformedCriticalTag {
        name: String,
        expected: usize,
        actual: usize,
    },
    #[error("duplicate or ambiguous critical manifest tag {0}")]
    DuplicateCriticalTag(String),
    #[error("manifest must contain exactly one [\"x\", hash, \"aggregate\"] tag")]
    DuplicateOrInvalidAggregate,
    #[error("manifest has no aggregate x tag")]
    MissingAggregate,
    #[error("invalid requires domain {0:?}")]
    InvalidRequirement(String),
    #[error("requires domain {0:?} is outside the pinned compatibility inventory")]
    UnknownRequirement(String),
    #[error("manifest declares {actual} requirements; the maximum is {maximum}")]
    RequirementCount { actual: usize, maximum: usize },
    #[error("manifest declares {actual} blob sources; the maximum is {maximum}")]
    SourceCount { actual: usize, maximum: usize },
    #[error("manifest source metadata is not an absolute credential-free HTTP(S) URL")]
    InvalidSourceUrl,
    #[error("blob server URL violates source policy")]
    InvalidBlobServer,
    #[error("no policy-approved blob source is available")]
    NoApprovedBlobSource,
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
}
