//! Bounded fact rings that never hide what they dropped.
//!
//! The rule these types exist to enforce: never present a bounded subset as a
//! complete answer. It is orthogonal to delta-versus-full — a full snapshot can
//! be honest and a delta stream can lie.
//!
//! Vocabulary, because the two honest counts are not the same number:
//!
//! - **dropped** is a cumulative eviction total. Those facts no longer exist
//!   anywhere; the ring destroyed them to stay bounded. That is what
//!   [`BoundedFacts::dropped`] counts.
//! - **omitted** is a per-projection count, as in the `omitted_*` fields of
//!   `crates/nmp-adapter/src/diagnostics.rs`. Those rows still exist; this
//!   particular view just did not carry them.
//!
//! Copy the honesty of that exemplar, not its noun.

use std::collections::VecDeque;

/// A capacity-bounded ring that remembers everything it was ever asked to hold.
///
/// `entries` is the retained tail a consumer gets to see. `appended` is a
/// monotonic count of every value ever pushed, so [`BoundedFacts::dropped`] is
/// exactly how far short of the whole truth that tail falls. A consumer that
/// reads `dropped() == 0` knows it is seeing a complete collection; any other
/// value is the count of facts that were evicted and are gone for good.
///
/// Both fields are private on purpose: every append goes through
/// [`BoundedFacts::push`], so nothing can add an entry without moving the
/// counter that keeps the ring honest.
///
/// `appended` is also a cheap change-detection counter: it strictly increases
/// on every push, so a consumer can compare two observations of it instead of
/// comparing the ring contents element by element.
#[derive(Debug)]
pub struct BoundedFacts<T> {
    entries: VecDeque<T>,
    appended: u64,
}

impl<T> BoundedFacts<T> {
    /// Preallocates the ring at its bound; the bound is still supplied per
    /// push so a single kernel limits struct stays the one owner of the cap.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            appended: 0,
        }
    }

    /// Appends `value`, evicting the oldest entry once `maximum` is reached.
    /// The eviction is counted, never silent.
    pub fn push(&mut self, maximum: usize, value: T) {
        if self.entries.len() == maximum {
            self.entries.pop_front();
        }
        self.entries.push_back(value);
        self.appended = self.appended.saturating_add(1);
    }

    /// Oldest retained entry, if any.
    pub fn front(&self) -> Option<&T> {
        self.entries.front()
    }

    /// Newest retained entry, if any.
    pub fn back(&self) -> Option<&T> {
        self.entries.back()
    }

    /// The retained tail, oldest first.
    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, T> {
        self.entries.iter()
    }

    /// How many entries are retained right now.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Everything ever pushed, including what has since been evicted.
    pub fn appended(&self) -> u64 {
        self.appended
    }

    /// How many facts the retained tail is short of the whole history.
    pub fn dropped(&self) -> u64 {
        self.appended.saturating_sub(self.entries.len() as u64)
    }
}

impl<'a, T> IntoIterator for &'a BoundedFacts<T> {
    type Item = &'a T;
    type IntoIter = std::collections::vec_deque::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::BoundedFacts;

    #[test]
    fn an_unfilled_ring_reports_nothing_dropped() {
        let mut facts = BoundedFacts::with_capacity(4);
        for value in 0..4_u64 {
            facts.push(4, value);
        }
        assert_eq!(facts.len(), 4);
        assert_eq!(facts.appended(), 4);
        assert_eq!(facts.dropped(), 0);
        assert_eq!(facts.iter().copied().collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn overflow_past_the_cap_reports_the_exact_omitted_count() {
        let mut facts = BoundedFacts::with_capacity(4);
        for value in 0..10_u64 {
            facts.push(4, value);
        }
        assert_eq!(facts.len(), 4);
        assert_eq!(facts.appended(), 10);
        assert_eq!(facts.dropped(), 6);
        assert_eq!(facts.iter().copied().collect::<Vec<_>>(), vec![6, 7, 8, 9]);
    }

    #[test]
    fn a_single_slot_ring_drops_every_predecessor() {
        let mut facts = BoundedFacts::with_capacity(1);
        for value in 0..3_u64 {
            facts.push(1, value);
        }
        assert_eq!(facts.len(), 1);
        assert_eq!(facts.dropped(), 2);
        assert_eq!(facts.iter().copied().collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn an_empty_ring_is_a_complete_answer() {
        let facts = BoundedFacts::<u64>::with_capacity(8);
        assert!(facts.is_empty());
        assert_eq!(facts.appended(), 0);
        assert_eq!(facts.dropped(), 0);
    }
}
