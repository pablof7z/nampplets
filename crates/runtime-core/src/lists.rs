//! Ownership primitives for runtime-mediated list mutation.
//!
//! The runtime reasons about a list as an ordered set of `(tag, value)`
//! entries and nothing more. Everything that makes it a Nostr event — kind
//! encoding, signature, relays, durability — stays behind
//! [`ListsDataPlane`], exactly as [`crate::HostDataPlane`] keeps canonical
//! write ownership out of the runtime.

use std::{fmt, sync::Arc};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{AccountRef, BoundedJson, Cancellation};

/// The tag an entry occupies in the replaceable list event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListItemTag {
    /// `p` — a public key.
    P,
    /// `e` — an event id.
    E,
    /// `t` — a hashtag.
    T,
    /// `a` — a replaceable-event address.
    A,
}

impl ListItemTag {
    pub const fn wire(self) -> &'static str {
        match self {
            Self::P => "p",
            Self::E => "e",
            Self::T => "t",
            Self::A => "a",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "p" => Some(Self::P),
            "e" => Some(Self::E),
            "t" => Some(Self::T),
            "a" => Some(Self::A),
            _ => None,
        }
    }
}

impl fmt::Display for ListItemTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.wire())
    }
}

/// One list identified exactly as NMP addresses it: kind plus, for a
/// parameterized replaceable list, its `d` identifier.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ListSelector {
    pub kind: u16,
    pub identifier: Option<Arc<str>>,
}

/// One entry's identity inside a list. Ordering and duplicate detection use
/// exactly this pair; positional extras (relay hints, petnames) belong to the
/// adapter and survive a mutation untouched.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ListEntry {
    pub tag: ListItemTag,
    pub value: Arc<str>,
}

impl ListEntry {
    pub fn new(tag: ListItemTag, value: impl Into<Arc<str>>) -> Self {
        Self {
            tag,
            value: value.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListReadLimits {
    pub maximum_entries: usize,
    pub maximum_frame_bytes: usize,
}

/// The exact current state of one replaceable list, scoped to a frozen
/// account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListSnapshot {
    /// Whether the account has ever published this list. A list that does not
    /// exist yet is created by the first mutation that changes something.
    pub exists: bool,
    /// The entries the runtime reasons about, in published order.
    pub entries: Vec<ListEntry>,
    /// Everything about the current event that the runtime must not interpret
    /// but must not destroy either: content, unrecognised tags, positional
    /// relay hints and petnames. Opaque here; the adapter reads it back when
    /// rendering the replacement.
    pub retained: BoundedJson,
}

/// Public-facade seam for list mutation, implemented by `nmp-adapter`.
///
/// No NMP type crosses this interface. The adapter keeps ownership of the
/// canonical event, its kind/tag encoding, its signature and its relays; the
/// runtime hands it a decided entry set and never an event.
pub trait ListsDataPlane: Send + Sync + fmt::Debug {
    /// Freezes the active public account. `None` means no account is
    /// connected, which is a truthful refusal rather than an error.
    fn freeze_account(&self) -> Result<Option<AccountRef>, ListsDataError>;

    /// Reads one list, scoped to exactly the supplied frozen account.
    fn read_list(
        &self,
        account: &AccountRef,
        selector: &ListSelector,
        cancellation: &Cancellation,
        limits: ListReadLimits,
    ) -> Result<ListSnapshot, ListsDataError>;

    /// Renders the exact replacement draft in the adapter's governed
    /// public-facade format.
    ///
    /// The entry set is already decided. The adapter encodes it, restores
    /// everything in [`ListSnapshot::retained`], and returns a draft the
    /// runtime can carry through its single `accept_write` path.
    fn draft_replacement(
        &self,
        account: &AccountRef,
        selector: &ListSelector,
        snapshot: &ListSnapshot,
        entries: &[ListEntry],
        maximum_draft_bytes: usize,
    ) -> Result<BoundedJson, ListsDataError>;
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ListsDataError {
    #[error("the list source is closed")]
    Closed,
    #[error("the read was cancelled before it resolved")]
    Cancelled,
    #[error("the list source returned data this runtime cannot trust")]
    InvalidSourceData,
    #[error("the list has more entries than this runtime will project")]
    TooManyEntries,
    #[error("the replacement draft exceeds its configured byte limit")]
    DraftTooLarge,
    #[error("this runtime cannot address that list: {0}")]
    Unaddressable(Arc<str>),
}
