use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("runtime store limits must be finite and non-zero")]
    InvalidLimits,
    #[error("unsupported runtime store schema {0}")]
    UnsupportedSchema(i64),
    #[error("runtime store is corrupt: {0}")]
    Corrupt(String),
    #[error("invalid {field}")]
    InvalidName { field: &'static str },
    #[error("installation capacity {capacity} is full")]
    InstallCapacity { capacity: usize },
    #[error("installation title must be non-empty and contain no control characters")]
    InvalidInstallTitle,
    #[error("installation title is {actual} bytes; the maximum is {maximum}")]
    InstallTitleTooLarge { actual: usize, maximum: usize },
    #[error("installation search query contains a control character")]
    InvalidInstallSearchQuery,
    #[error("installation search query is {actual} bytes; the maximum is {maximum}")]
    InstallSearchQueryTooLarge { actual: usize, maximum: usize },
    #[error("installation search limit {requested} is invalid; it must be between 1 and {maximum}")]
    InvalidInstallSearchLimit { requested: usize, maximum: usize },
    #[error(
        "installation search has at least {actual_at_least} results; the response maximum is {maximum}"
    )]
    InstallSearchCapacity {
        actual_at_least: usize,
        maximum: usize,
    },
    #[error("manifest metadata is {actual} bytes; the maximum is {maximum}")]
    ManifestMetadataTooLarge { actual: usize, maximum: usize },
    #[error("installation was not found")]
    InstallationNotFound,
    #[error("capability request count {actual} exceeds the maximum {maximum}")]
    CapabilityRequestCapacity { actual: usize, maximum: usize },
    #[error("installed capability requests repeat a domain")]
    DuplicateCapabilityRequest,
    #[error("grant capacity {capacity} is full for this exact principal")]
    GrantCapacity { capacity: usize },
    #[error("grant decision batch must not be empty")]
    EmptyGrantBatch,
    #[error("grant decision batch repeats a capability")]
    DuplicateGrantBatchCapability,
    #[error("component value is {actual} bytes; the maximum is {maximum}")]
    ValueTooLarge { actual: usize, maximum: usize },
    #[error("component key capacity {capacity} is full for this exact scope")]
    KeyCapacity { capacity: usize },
    #[error("component key-list limit {requested} is invalid; it must be between 1 and {maximum}")]
    InvalidKeyListLimit { requested: usize, maximum: usize },
    #[error(
        "component scope has at least {actual_at_least} keys; the response maximum is {maximum}"
    )]
    KeyListCapacity {
        actual_at_least: usize,
        maximum: usize,
    },
    #[error("component scope would use {actual} bytes; the maximum is {maximum}")]
    ScopeBytes { actual: usize, maximum: usize },
    #[error("workspace capacity {capacity} is full")]
    WorkspaceCapacity { capacity: usize },
    #[error("workspace was not found")]
    WorkspaceNotFound,
    #[error("workspace assignment capacity {capacity} is full")]
    WorkspaceAssignmentCapacity { capacity: usize },
    #[error("workspace is {actual} bytes; the maximum is {maximum}")]
    WorkspaceTooLarge { actual: usize, maximum: usize },
    #[error("workspace retains {actual} receipts; the maximum is {maximum}")]
    RetainedReceiptCapacity { actual: usize, maximum: usize },
    #[error("workspace retained receipt references use {actual} bytes; the maximum is {maximum}")]
    RetainedReceiptBytes { actual: usize, maximum: usize },
    #[error("activity {field} must be non-empty and contain no control characters")]
    InvalidActivityString { field: &'static str },
    #[error("activity {field} is {actual} bytes; the maximum is {maximum}")]
    ActivityStringTooLarge {
        field: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("activity record strings use {actual} bytes; the maximum is {maximum}")]
    ActivityRecordTooLarge { actual: usize, maximum: usize },
    #[error("profile relay lane {lane} has {actual} entries; the maximum is {maximum}")]
    ProfileRelayCapacity {
        lane: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("profile relay URL in {lane} is invalid")]
    InvalidProfileRelay { lane: &'static str },
    #[error("profile relay URL in {lane} is {actual} bytes; the maximum is {maximum}")]
    ProfileRelayTooLarge {
        lane: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("profile relay lane {lane} contains a duplicate URL")]
    DuplicateProfileRelay { lane: &'static str },
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}
