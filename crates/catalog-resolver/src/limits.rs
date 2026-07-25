use crate::ResolveError;

/// Finite resolver-wide limits. Every value must be non-zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolverLimits {
    pub maximum_in_flight: usize,
    pub maximum_reviews: usize,
    pub maximum_lookup_facts: usize,
    pub maximum_acquisition_facts: usize,
    pub maximum_manifest_bytes: usize,
    pub maximum_resolved_addresses: usize,
    pub maximum_url_bytes: usize,
    pub maximum_source_label_bytes: usize,
    pub maximum_reason_bytes: usize,
    /// Redirect hops followed per candidate before acquisition refuses. Each
    /// hop target is revalidated with the same HTTPS-only, credential-free,
    /// public-address policy as the original candidate URL.
    pub maximum_redirect_hops: usize,
}

impl Default for ResolverLimits {
    fn default() -> Self {
        Self {
            maximum_in_flight: 4,
            maximum_reviews: 16,
            maximum_lookup_facts: 64,
            maximum_acquisition_facts: 4_096,
            maximum_manifest_bytes: 256 * 1_024,
            maximum_resolved_addresses: 16,
            maximum_url_bytes: 2_048,
            maximum_source_label_bytes: 256,
            maximum_reason_bytes: 512,
            maximum_redirect_hops: 5,
        }
    }
}

impl ResolverLimits {
    pub(crate) fn validate(self) -> Result<Self, ResolveError> {
        if self.maximum_in_flight == 0
            || self.maximum_reviews == 0
            || self.maximum_lookup_facts == 0
            || self.maximum_acquisition_facts == 0
            || self.maximum_manifest_bytes == 0
            || self.maximum_resolved_addresses == 0
            || self.maximum_url_bytes == 0
            || self.maximum_source_label_bytes == 0
            || self.maximum_reason_bytes == 0
            || self.maximum_redirect_hops == 0
        {
            return Err(ResolveError::InvalidLimits);
        }
        Ok(self)
    }
}
