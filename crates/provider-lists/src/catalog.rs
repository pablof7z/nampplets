use crate::ListItemTag;

/// One list this runtime can actually mutate.
///
/// The catalog is pinned in Rust and is the single answer to
/// `lists.supported`. A kind absent from it is refused, never attempted — the
/// runtime does not guess at the shape of a list it has no contract for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupportedList {
    pub kind: u16,
    /// Stable machine name a napplet can match on.
    pub name: &'static str,
    /// Item tags this list accepts. An item carrying any other tag is refused.
    pub item_types: &'static [ListItemTag],
    /// Parameterized replaceable lists (30000-39999) are addressed by a `d`
    /// identifier; the rest must not carry one.
    pub parameterized: bool,
}

/// NIP-51 lists (plus the NIP-02 follow list) this runtime services.
///
/// Deliberately conservative: every entry here is a replaceable list whose
/// public tag set is the whole of its meaning, so a mutation is a pure
/// set operation. Lists whose semantics live in encrypted content are absent
/// rather than half-supported.
pub const SUPPORTED_LISTS: &[SupportedList] = &[
    SupportedList {
        kind: 3,
        name: "follows",
        item_types: &[ListItemTag::P],
        parameterized: false,
    },
    SupportedList {
        kind: 10_000,
        name: "mute",
        item_types: &[ListItemTag::P, ListItemTag::E, ListItemTag::T],
        parameterized: false,
    },
    SupportedList {
        kind: 10_001,
        name: "pin",
        item_types: &[ListItemTag::E],
        parameterized: false,
    },
    SupportedList {
        kind: 10_003,
        name: "bookmark",
        item_types: &[ListItemTag::E, ListItemTag::A, ListItemTag::T],
        parameterized: false,
    },
    SupportedList {
        kind: 10_015,
        name: "interest",
        item_types: &[ListItemTag::T],
        parameterized: false,
    },
    SupportedList {
        kind: 30_000,
        name: "follow-set",
        item_types: &[ListItemTag::P],
        parameterized: true,
    },
    SupportedList {
        kind: 30_003,
        name: "bookmark-set",
        item_types: &[ListItemTag::E, ListItemTag::A, ListItemTag::T],
        parameterized: true,
    },
    SupportedList {
        kind: 30_015,
        name: "interest-set",
        item_types: &[ListItemTag::T],
        parameterized: true,
    },
];

pub fn supported_list(kind: u16) -> Option<&'static SupportedList> {
    SUPPORTED_LISTS.iter().find(|list| list.kind == kind)
}

impl SupportedList {
    pub fn accepts(&self, tag: ListItemTag) -> bool {
        self.item_types.contains(&tag)
    }
}
