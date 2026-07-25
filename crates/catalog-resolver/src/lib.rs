//! Bounded coordinate lookup and policy-checked artifact acquisition.
//!
//! NMP selects one canonical manifest event through [`ManifestLookupPort`].
//! This crate validates finite lookup evidence and raw HTTPS acquisition facts,
//! then delegates all signature, manifest, path-hash, aggregate, and immutable
//! byte handling to `nmp-native-artifact`.

mod cache;
mod cancellation;
mod error;
mod https;
mod limits;
mod lookup;
mod redirect;
mod resolver;
mod review;

pub use cache::{
    MemorySealedArtifactCache, ResolutionOrigin, ResolvedArtifact, SealedArtifactCache,
    SealedArtifactKey, SealedCacheError,
};
pub use cancellation::CancellationToken;
pub use error::ResolveError;
pub use https::{
    AcquisitionFact, AcquisitionOutcome, AcquisitionRefusal, HttpsAcquisitionCompletion,
    HttpsAcquisitionOperation, HttpsAcquisitionPort, HttpsFetchRequest, HttpsFetchResponse,
    HttpsPortError, RustHttpsAcquisitionConfig, RustHttpsAcquisitionPort,
};
pub use limits::ResolverLimits;
pub use lookup::{
    CoordinateLookupFact, CoordinateLookupState, LookupPortError, ManifestLookupCompletion,
    ManifestLookupOperation, ManifestLookupPort, ManifestLookupRequest, ManifestLookupResponse,
};
pub use resolver::CatalogResolver;
pub use review::{ArtifactReview, ArtifactReviewSummary};

#[cfg(test)]
mod tests;
