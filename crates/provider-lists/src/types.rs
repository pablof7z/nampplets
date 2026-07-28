/// Runtime-owned list primitives. The port and its value types live in
/// `runtime-core` so the NMP adapter can implement them without depending on
/// this provider, exactly as it does for public identity.
pub use nmp_native_runtime_core::{
    ListEntry, ListItemTag, ListReadLimits, ListSelector, ListSnapshot, ListsDataError,
    ListsDataPlane,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListsProviderLimits {
    pub maximum_sessions: usize,
    pub maximum_response_bytes: usize,
    pub maximum_draft_bytes: usize,
    pub maximum_correlation_id_bytes: usize,
    /// Items accepted in one `add`/`remove` request.
    pub maximum_request_items: usize,
    /// Entries the resulting list may hold. A mutation that would cross this
    /// bound is refused whole rather than silently truncated.
    pub maximum_list_entries: usize,
    pub maximum_identifier_bytes: usize,
    pub maximum_value_bytes: usize,
}

impl Default for ListsProviderLimits {
    fn default() -> Self {
        Self {
            maximum_sessions: 64,
            maximum_response_bytes: 256 * 1024,
            maximum_draft_bytes: 512 * 1024,
            maximum_correlation_id_bytes: 1_024,
            maximum_request_items: 256,
            maximum_list_entries: 4_096,
            maximum_identifier_bytes: 256,
            maximum_value_bytes: 1_024,
        }
    }
}

/// What one `add`/`remove` actually did, decided in Rust before any write is
/// proposed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListMutation {
    /// The exact entry set the replacement event must carry.
    pub entries: Vec<ListEntry>,
    /// Requested items that changed the list.
    pub changed: usize,
    /// Requested items that were already in the requested state.
    pub skipped: usize,
}

impl ListMutation {
    pub fn is_noop(&self) -> bool {
        self.changed == 0
    }
}
