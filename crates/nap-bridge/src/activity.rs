use std::{collections::VecDeque, fmt, sync::Arc};

use nmp_native_runtime_core::{Capability, Principal, SessionId};
use parking_lot::Mutex;

pub trait ActivitySink: Send + Sync + fmt::Debug {
    fn record(&self, fact: ProviderActivity);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderActivity {
    pub principal: Principal,
    pub session: SessionId,
    pub domain: Capability,
    pub action: Arc<str>,
    pub outcome: ActivityOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityOutcome {
    Completed,
    Active,
    Refused,
}

#[derive(Debug, Default)]
pub struct MemoryActivitySink {
    maximum: usize,
    facts: Mutex<VecDeque<ProviderActivity>>,
}

impl MemoryActivitySink {
    pub fn bounded(maximum: usize) -> Self {
        Self {
            maximum,
            facts: Mutex::new(VecDeque::with_capacity(maximum)),
        }
    }

    pub fn facts(&self) -> Vec<ProviderActivity> {
        self.facts.lock().iter().cloned().collect()
    }
}

impl ActivitySink for MemoryActivitySink {
    fn record(&self, fact: ProviderActivity) {
        if self.maximum == 0 {
            return;
        }
        let mut facts = self.facts.lock();
        if facts.len() == self.maximum {
            facts.pop_front();
        }
        facts.push_back(fact);
    }
}
