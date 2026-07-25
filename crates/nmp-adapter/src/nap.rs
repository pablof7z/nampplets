//! Bounded NAP-OUTBOX and NAP-RELAY providers backed exclusively by the
//! supported NMP facade.
//!
//! The provider retains only session-scoped delivery state. NMP remains the
//! sole owner of canonical events, relay planning, signing, durable write
//! intents, pending rows, and receipts.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    num::NonZeroUsize,
    str::FromStr,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use nmp::{
    AccessContext, Binding, CacheMode, Demand, Engine, Filter, Freshness, IndexedTagName,
    LiveQuery, ObservationCancel, PublicKey, RelayUrl, Row, SourceAuthority, UnsignedEvent, Window,
    WindowLoad,
};
use nmp_native_nap_bridge::{
    Provider, ProviderCall, ProviderDescriptor, ProviderError, ProviderPlatformAvailability,
    ProviderPushSender, ProviderRequest, ProviderSession, ProviderSessionContext,
    ProviderSessionEnd, ProviderWriteCompletion,
};
use nmp_native_runtime_core::{
    AccountRef, ApprovedWrite, BoundedJson, Capability, Principal, PublicIdentityDataPlane,
    ReceiptEventSink, ReceiptSinkError, ReceiptSnapshot, SessionId,
};
use parking_lot::Mutex;
use serde_json::{Map, Value, json};

use super::NmpDataPlane;

pub const OUTBOX_DOMAIN: &str = "outbox";
pub const RELAY_DOMAIN: &str = "relay";
const PINNED_NAP_PROTOCOL: &str = "napplet-web@0.28.0";
const RECEIPT_EVENT_LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NapNostrProviderLimits {
    pub maximum_sessions: usize,
    pub maximum_subscriptions_per_session: usize,
    pub maximum_filters: usize,
    pub maximum_events: usize,
    pub maximum_seen_event_ids: usize,
    pub maximum_relays: usize,
    pub maximum_tags: usize,
    pub maximum_tag_values: usize,
    pub maximum_text_bytes: usize,
    pub maximum_response_bytes: usize,
    pub maximum_correlation_id_bytes: usize,
    pub maximum_subscription_id_bytes: usize,
    pub default_query_timeout_millis: u64,
    pub maximum_query_timeout_millis: u64,
}

impl Default for NapNostrProviderLimits {
    fn default() -> Self {
        Self {
            maximum_sessions: 64,
            maximum_subscriptions_per_session: 32,
            maximum_filters: 64,
            maximum_events: 1_024,
            maximum_seen_event_ids: 4_096,
            maximum_relays: 64,
            maximum_tags: 128,
            maximum_tag_values: 64,
            maximum_text_bytes: 64 * 1024,
            maximum_response_bytes: 512 * 1024,
            maximum_correlation_id_bytes: 64,
            maximum_subscription_id_bytes: 256,
            default_query_timeout_millis: 2_000,
            maximum_query_timeout_millis: 10_000,
        }
    }
}

#[derive(Debug)]
pub struct NapNostrProviderSet {
    pub outbox: Arc<NapNostrProvider>,
    pub relay: Arc<NapNostrProvider>,
}

impl NapNostrProviderSet {
    pub fn new(
        plane: Arc<NmpDataPlane>,
        limits: NapNostrProviderLimits,
    ) -> Result<Self, ProviderError> {
        validate_limits(limits)?;
        Ok(Self {
            outbox: NapNostrProvider::new(Arc::clone(&plane), NapDomain::Outbox, limits),
            relay: NapNostrProvider::new(plane, NapDomain::Relay, limits),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NapDomain {
    Outbox,
    Relay,
}

impl NapDomain {
    fn name(self) -> &'static str {
        match self {
            Self::Outbox => OUTBOX_DOMAIN,
            Self::Relay => RELAY_DOMAIN,
        }
    }

    fn event_type(self) -> &'static str {
        match self {
            Self::Outbox => "outbox.event",
            Self::Relay => "relay.event",
        }
    }

    fn closed_type(self) -> &'static str {
        match self {
            Self::Outbox => "outbox.closed",
            Self::Relay => "relay.closed",
        }
    }
}

pub struct NapNostrProvider {
    plane: Arc<NmpDataPlane>,
    domain: NapDomain,
    limits: NapNostrProviderLimits,
    descriptor: ProviderDescriptor,
    state: Arc<Mutex<ProviderState>>,
}

impl fmt::Debug for NapNostrProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.lock();
        formatter
            .debug_struct("NapNostrProvider")
            .field("domain", &self.domain)
            .field("sessions", &state.sessions.len())
            .field("closed", &state.closed)
            .finish()
    }
}

#[derive(Debug, Default)]
struct ProviderState {
    sessions: BTreeMap<SessionId, NapSession>,
    closed: bool,
}

#[derive(Debug)]
struct NapSession {
    principal: Principal,
    source_window: nmp_native_nap_bridge::SourceWindowId,
    outbound: ProviderPushSender,
    subscriptions: BTreeMap<Arc<str>, ActiveSubscription>,
}

struct ActiveSubscription {
    cancels: Vec<ObservationCancel>,
}

impl fmt::Debug for ActiveSubscription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActiveSubscription")
            .field("observations", &self.cancels.len())
            .finish()
    }
}

impl NapNostrProvider {
    fn new(
        plane: Arc<NmpDataPlane>,
        domain: NapDomain,
        limits: NapNostrProviderLimits,
    ) -> Arc<Self> {
        let actions = match domain {
            NapDomain::Outbox => [
                "getEvent",
                "query",
                "subscribe",
                "close",
                "publish",
                "resolveRelays",
            ]
            .as_slice(),
            NapDomain::Relay => {
                ["query", "subscribe", "close", "publish", "publishEncrypted"].as_slice()
            }
        };
        Arc::new(Self {
            plane,
            domain,
            limits,
            descriptor: ProviderDescriptor {
                domain: Capability::new(domain.name())
                    .expect("static NAP Nostr capability is valid"),
                protocol_versions: BTreeSet::from([Arc::from(PINNED_NAP_PROTOCOL)]),
                actions: actions.iter().copied().map(Arc::from).collect(),
                sensitive: true,
                dependencies: BTreeSet::new(),
                platform_availability: ProviderPlatformAvailability::Available,
            },
            state: Arc::new(Mutex::new(ProviderState::default())),
        })
    }

    pub fn close(&self) {
        let subscriptions = {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            state.closed = true;
            state
                .sessions
                .values_mut()
                .flat_map(|session| {
                    std::mem::take(&mut session.subscriptions)
                        .into_values()
                        .flat_map(|subscription| subscription.cancels)
                })
                .collect::<Vec<_>>()
        };
        for cancel in subscriptions {
            cancel.cancel();
        }
        self.state.lock().sessions.clear();
    }

    fn session_outbound(
        &self,
        request: &ProviderRequest,
    ) -> Result<ProviderPushSender, ProviderError> {
        let state = self.state.lock();
        let session = state
            .sessions
            .get(&request.session)
            .ok_or_else(|| self.denied(request, "provider session is not open"))?;
        if session.principal != request.principal {
            return Err(self.denied(request, "provider session principal does not match"));
        }
        Ok(session.outbound.clone())
    }

    fn query(
        &self,
        request: ProviderRequest,
        get_event: bool,
    ) -> Result<ProviderCall, ProviderError> {
        let id: Arc<str> = Arc::from(correlation_id(&request, self.limits)?);
        let outbound = self.session_outbound(&request)?;
        let payload = object_payload(&request)?;
        let (filters, source, timeout) = if get_event {
            let event_id = required_string(payload, "eventId", &request)?;
            validate_hex(event_id, 64, "eventId", &request)?;
            let options = optional_object(payload, "options", &request)?;
            validate_exact_keys(payload, &["eventId", "options"], &request)?;
            let author = options
                .and_then(|value| value.get("author"))
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| self.invalid(&request, "options.author must be a string"))
                })
                .transpose()?;
            if let Some(author) = author {
                validate_hex(author, 64, "options.author", &request)?;
            }
            let relays = parse_relay_hints(options, self.limits, &request)?;
            let timeout = parse_timeout(options, self.limits, &request)?;
            let filter = NapFilter {
                ids: Some(BTreeSet::from([event_id.to_owned()])),
                authors: author.map(|value| BTreeSet::from([value.to_owned()])),
                ..NapFilter::default()
            };
            let source = read_source(
                self.domain,
                std::slice::from_ref(&filter),
                &[],
                relays,
                &request,
            )?;
            (vec![filter], source, timeout)
        } else {
            validate_exact_keys(payload, &["filters", "options"], &request)?;
            let filters = parse_filters(
                payload
                    .get("filters")
                    .ok_or_else(|| self.invalid(&request, "filters is required"))?,
                self.limits,
                self.domain == NapDomain::Outbox,
                &request,
            )?;
            let options = optional_object(payload, "options", &request)?;
            let authors = parse_author_hints(options, self.limits, &request)?;
            let relays = parse_relay_hints(options, self.limits, &request)?;
            let requested_limit = parse_query_limit(options, self.limits, &request)?;
            let timeout = parse_timeout(options, self.limits, &request)?;
            let source = read_source(self.domain, &filters, &authors, relays, &request)?;
            let filters = cap_filter_limits(filters, requested_limit);
            (filters, source, timeout)
        };
        let demand = broad_demand(&filters, source, &request)?;
        let permit = self
            .plane
            .workers
            .reserve("nmp-nap-query")
            .map_err(|error| self.failed(&request, error.to_string()))?;
        let engine = Arc::clone(&self.plane.engine);
        let domain = self.domain;
        let limits = self.limits;
        let cancellation = request.work.cancellation().clone();
        let action = Arc::clone(&request.action);
        let work = request.work;
        let error_domain: Arc<str> = Arc::from(domain.name());
        thread::Builder::new()
            .name("nmp-nap-query".to_owned())
            .spawn(move || {
                let _permit = permit;
                let _work = work;
                let projection =
                    read_once(&engine, demand, &filters, timeout, limits, &cancellation);
                let value = query_result(domain, &id, get_event, projection);
                let _ = push_value(&outbound, value, limits.maximum_response_bytes, None);
            })
            .map_err(|error| ProviderError::Failed {
                domain: error_domain,
                action,
                reason: Arc::from(error.to_string()),
            })?;
        Ok(ProviderCall::completed(None))
    }

    fn subscribe(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        let _id = correlation_id(&request, self.limits)?;
        let outbound = self.session_outbound(&request)?;
        let payload = object_payload(&request)?;
        let allowed = match self.domain {
            NapDomain::Outbox => &["subId", "filters", "options"][..],
            NapDomain::Relay => &["subId", "filters", "relay"][..],
        };
        validate_exact_keys(payload, allowed, &request)?;
        let sub_id = required_string(payload, "subId", &request)?;
        validate_sub_id(sub_id, self.limits, &request)?;
        let filters = parse_filters(
            payload
                .get("filters")
                .ok_or_else(|| self.invalid(&request, "filters is required"))?,
            self.limits,
            self.domain == NapDomain::Outbox,
            &request,
        )?;
        let (authors, relays) = match self.domain {
            NapDomain::Outbox => {
                let options = optional_object(payload, "options", &request)?;
                (
                    parse_author_hints(options, self.limits, &request)?,
                    parse_relay_hints(options, self.limits, &request)?,
                )
            }
            NapDomain::Relay => {
                let relays = payload
                    .get("relay")
                    .map(|value| {
                        let relay = value
                            .as_str()
                            .ok_or_else(|| self.invalid(&request, "relay must be a string"))?;
                        parse_relays(std::slice::from_ref(&relay), self.limits, &request)
                    })
                    .transpose()?
                    .unwrap_or_default();
                (Vec::new(), relays)
            }
        };
        let source = read_source(self.domain, &filters, &authors, relays, &request)?;
        let demand = broad_demand(&filters, source, &request)?;
        let maximum = NonZeroUsize::new(self.limits.maximum_events)
            .expect("validated provider event maximum is non-zero");
        let subscription = self
            .plane
            .engine
            .observe(
                LiveQuery(demand),
                Some(Window::Expandable {
                    initial: maximum,
                    max: maximum,
                }),
            )
            .map_err(|error| self.failed(&request, error.to_string()))?;
        let cancel = subscription.cancel_handle();
        let permit = self
            .plane
            .workers
            .reserve("nmp-nap-subscription")
            .map_err(|error| {
                cancel.cancel();
                self.failed(&request, error.to_string())
            })?;
        let sub_id: Arc<str> = Arc::from(sub_id);
        {
            let mut state = self.state.lock();
            let session = state
                .sessions
                .get_mut(&request.session)
                .ok_or_else(|| self.denied(&request, "provider session is not open"))?;
            if session.subscriptions.contains_key(&sub_id) {
                cancel.cancel();
                return Err(self.denied(&request, "subscription id is already active"));
            }
            if session.subscriptions.len() >= self.limits.maximum_subscriptions_per_session {
                cancel.cancel();
                return Err(self.denied(
                    &request,
                    format!(
                        "subscription capacity {} is full",
                        self.limits.maximum_subscriptions_per_session
                    ),
                ));
            }
            session.subscriptions.insert(
                Arc::clone(&sub_id),
                ActiveSubscription {
                    cancels: vec![cancel.clone()],
                },
            );
        }
        let domain = self.domain;
        let limits = self.limits;
        let cancellation = request.work.cancellation().clone();
        let work = request.work;
        let state = Arc::downgrade(&self.state);
        let principal = request.principal.clone();
        let session_id = request.session;
        let worker_sub_id = Arc::clone(&sub_id);
        let spawn = thread::Builder::new()
            .name("nmp-nap-subscription".to_owned())
            .spawn(move || {
                let _permit = permit;
                let _work = work;
                drain_subscription(
                    subscription,
                    domain,
                    &outbound,
                    &worker_sub_id,
                    &filters,
                    limits,
                    &cancellation,
                );
                remove_finished_subscription(state, session_id, &principal, &worker_sub_id);
            });
        if let Err(error) = spawn {
            cancel.cancel();
            remove_finished_subscription(
                Arc::downgrade(&self.state),
                request.session,
                &request.principal,
                &sub_id,
            );
            return Err(ProviderError::Failed {
                domain: Arc::from(self.domain.name()),
                action: Arc::from("subscribe"),
                reason: Arc::from(error.to_string()),
            });
        }
        Ok(ProviderCall::completed(None))
    }

    fn close_subscription(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        let _id = correlation_id(&request, self.limits)?;
        let payload = object_payload(&request)?;
        validate_exact_keys(payload, &["subId"], &request)?;
        let sub_id = required_string(payload, "subId", &request)?;
        validate_sub_id(sub_id, self.limits, &request)?;
        let (subscription, outbound) = {
            let mut state = self.state.lock();
            let session = state
                .sessions
                .get_mut(&request.session)
                .ok_or_else(|| self.denied(&request, "provider session is not open"))?;
            if session.principal != request.principal {
                return Err(self.denied(&request, "provider session principal does not match"));
            }
            (
                session.subscriptions.remove(sub_id),
                session.outbound.clone(),
            )
        };
        if let Some(subscription) = subscription {
            for cancel in subscription.cancels {
                cancel.cancel();
            }
            let mut fields = Map::new();
            fields.insert("subId".to_owned(), Value::String(sub_id.to_owned()));
            fields.insert("reason".to_owned(), Value::String("closed".to_owned()));
            let _ = outbound.push(self.domain.closed_type(), fields, Some(sub_id));
        }
        Ok(ProviderCall::completed(None))
    }

    fn publish(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        let id: Arc<str> = Arc::from(correlation_id(&request, self.limits)?);
        let outbound = self.session_outbound(&request)?;
        let payload = object_payload(&request)?;
        let allowed = match self.domain {
            NapDomain::Outbox => &["event", "options"][..],
            NapDomain::Relay => &["event"][..],
        };
        validate_exact_keys(payload, allowed, &request)?;
        if self.domain == NapDomain::Outbox {
            if let Err(error) = validate_publish_options(
                optional_object(payload, "options", &request)?,
                self.limits,
                &request,
            ) {
                return self.completed_value(
                    &request,
                    json!({
                        "type": "outbox.publish.result",
                        "id": &*id,
                        "ok": false,
                        "error": error.to_string(),
                    }),
                );
            }
        }
        let event = payload
            .get("event")
            .ok_or_else(|| self.invalid(&request, "event is required"))?;
        let (draft, account, approval_id) = if event.get("sig").is_some() {
            let signed: nmp::Event = serde_json::from_value(event.clone()).map_err(|error| {
                self.invalid(&request, format!("invalid signed event: {error}"))
            })?;
            signed.verify().map_err(|error| {
                self.invalid(&request, format!("invalid signed event: {error}"))
            })?;
            (
                BoundedJson::from_value(
                    &serde_json::to_value(&signed)
                        .map_err(|error| self.invalid(&request, error.to_string()))?,
                    self.limits.maximum_response_bytes,
                )
                .map_err(|error| self.invalid(&request, error.to_string()))?,
                AccountRef(Arc::from(signed.pubkey.to_string())),
                Arc::from(signed.id.to_string()),
            )
        } else {
            let identity = self
                .plane
                .freeze_public_identity()
                .map_err(|error| self.failed(&request, error.to_string()))?;
            let account = identity
                .account
                .ok_or_else(|| self.denied(&request, "publishing requires an active account"))?;
            let mut unsigned = parse_event_template(event, &account, self.limits, &request)?;
            let approval_id: Arc<str> = Arc::from(unsigned.id().to_string());
            (
                BoundedJson::from_value(
                    &serde_json::to_value(&unsigned)
                        .map_err(|error| self.invalid(&request, error.to_string()))?,
                    self.limits.maximum_response_bytes,
                )
                .map_err(|error| self.invalid(&request, error.to_string()))?,
                account,
                approval_id,
            )
        };
        let write = ApprovedWrite {
            approval_id,
            origin_principal: request.principal.clone(),
            origin_session: request.session,
            account,
            draft,
        };
        let completion = Box::new(NapPublishCompletion {
            domain: self.domain,
            id,
            outbound,
            engine: Arc::clone(&self.plane.engine),
            maximum_response_bytes: self.limits.maximum_response_bytes,
        });
        Ok(ProviderCall::proposed_write(
            None,
            write,
            completion,
            request.work,
        ))
    }

    fn publish_encrypted(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        let id: Arc<str> = Arc::from(correlation_id(&request, self.limits)?);
        let payload = object_payload(&request)?;
        validate_exact_keys(payload, &["event", "recipient", "encryption"], &request)?;
        required_string(payload, "recipient", &request)?;
        self.completed_value(
            &request,
            json!({
                "type": "relay.publishEncrypted.result",
                "id": id,
                "ok": false,
                "error": "the pinned NMP public facade does not expose governed content encryption",
            }),
        )
    }

    fn resolve_relays(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        let id: Arc<str> = Arc::from(correlation_id(&request, self.limits)?);
        let outbound = self.session_outbound(&request)?;
        let payload = object_payload(&request)?;
        validate_exact_keys(payload, &["target"], &request)?;
        let target = payload
            .get("target")
            .and_then(Value::as_object)
            .ok_or_else(|| self.invalid(&request, "target must be an object"))?;
        if target
            .keys()
            .any(|key| !["authors", "pubkey", "direction"].contains(&key.as_str()))
        {
            return Err(self.invalid(&request, "target contains unknown fields"));
        }
        let mut authors = target
            .get("authors")
            .map(|value| {
                parse_string_array(
                    value,
                    self.limits.maximum_filters,
                    "target.authors",
                    &request,
                )
            })
            .transpose()?
            .unwrap_or_default();
        if let Some(pubkey) = target.get("pubkey") {
            let pubkey = pubkey
                .as_str()
                .ok_or_else(|| self.invalid(&request, "target.pubkey must be a string"))?;
            authors.push(pubkey.to_owned());
        }
        authors.sort();
        authors.dedup();
        if authors.is_empty() {
            return self.completed_value(
                &request,
                json!({
                    "type": "outbox.resolveRelays.result",
                    "id": &*id,
                    "plan": {"relays": [], "source": "fallback", "missingAuthors": []},
                    "error": "target requires at least one author",
                }),
            );
        }
        for author in &authors {
            validate_hex(author, 64, "target author", &request)?;
        }
        let direction = target
            .get("direction")
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| self.invalid(&request, "target.direction must be a string"))
            })
            .transpose()?
            .unwrap_or("read")
            .to_owned();
        if !matches!(direction.as_str(), "read" | "write") {
            return Err(self.invalid(&request, "target.direction must be read or write"));
        }
        if direction == "write" {
            return self.completed_value(
                &request,
                json!({
                    "type": "outbox.resolveRelays.result",
                    "id": &*id,
                    "plan": {
                        "relays": [],
                        "source": "fallback",
                        "missingAuthors": authors,
                    },
                    "error": "the pinned NMP app facade does not expose inbox route-plan inspection",
                }),
            );
        }
        let filter = NapFilter {
            kinds: Some(BTreeSet::from([10_002])),
            authors: Some(authors.iter().cloned().collect()),
            limit: Some(authors.len()),
            ..NapFilter::default()
        };
        let demand = broad_demand(
            std::slice::from_ref(&filter),
            SourceChoice::AuthorOutboxes,
            &request,
        )?;
        let permit = self
            .plane
            .workers
            .reserve("nmp-nap-resolve-relays")
            .map_err(|error| self.failed(&request, error.to_string()))?;
        let engine = Arc::clone(&self.plane.engine);
        let limits = self.limits;
        let cancellation = request.work.cancellation().clone();
        let action = Arc::clone(&request.action);
        let work = request.work;
        let error_domain: Arc<str> = Arc::from(self.domain.name());
        thread::Builder::new()
            .name("nmp-nap-resolve-relays".to_owned())
            .spawn(move || {
                let _permit = permit;
                let _work = work;
                let projection = read_once(
                    &engine,
                    demand,
                    std::slice::from_ref(&filter),
                    Duration::from_millis(limits.default_query_timeout_millis),
                    limits,
                    &cancellation,
                );
                let value = resolve_result(&id, &authors, &direction, projection);
                let _ = push_value(&outbound, value, limits.maximum_response_bytes, None);
            })
            .map_err(|error| ProviderError::Failed {
                domain: error_domain,
                action,
                reason: Arc::from(error.to_string()),
            })?;
        Ok(ProviderCall::completed(None))
    }

    fn completed_value(
        &self,
        request: &ProviderRequest,
        value: Value,
    ) -> Result<ProviderCall, ProviderError> {
        let response = BoundedJson::from_value(&value, self.limits.maximum_response_bytes)
            .map_err(|error| self.failed(request, error.to_string()))?;
        Ok(ProviderCall::completed(Some(response)))
    }

    fn invalid(&self, request: &ProviderRequest, reason: impl Into<Arc<str>>) -> ProviderError {
        ProviderError::InvalidPayload {
            domain: Arc::from(self.domain.name()),
            action: Arc::clone(&request.action),
            reason: reason.into(),
        }
    }

    fn denied(&self, request: &ProviderRequest, reason: impl Into<Arc<str>>) -> ProviderError {
        ProviderError::Denied {
            domain: Arc::from(self.domain.name()),
            action: Arc::clone(&request.action),
            reason: reason.into(),
        }
    }

    fn failed(&self, request: &ProviderRequest, reason: impl Into<Arc<str>>) -> ProviderError {
        ProviderError::Failed {
            domain: Arc::from(self.domain.name()),
            action: Arc::clone(&request.action),
            reason: reason.into(),
        }
    }
}

impl Provider for NapNostrProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    fn call(&self, request: ProviderRequest) -> Result<ProviderCall, ProviderError> {
        let result = match (self.domain, request.action.as_ref()) {
            (NapDomain::Outbox, "getEvent") => self.query(request, true),
            (NapDomain::Outbox | NapDomain::Relay, "query") => self.query(request, false),
            (NapDomain::Outbox | NapDomain::Relay, "subscribe") => self.subscribe(request),
            (NapDomain::Outbox | NapDomain::Relay, "close") => self.close_subscription(request),
            (NapDomain::Outbox | NapDomain::Relay, "publish") => self.publish(request),
            (NapDomain::Outbox, "resolveRelays") => self.resolve_relays(request),
            (NapDomain::Relay, "publishEncrypted") => self.publish_encrypted(request),
            _ => Err(self.invalid(&request, "unknown action")),
        };
        result.map_err(|error| normalize_error_domain(error, self.domain))
    }

    fn session_opened(&self, session: ProviderSession) -> Result<(), ProviderError> {
        let domain: Arc<str> = Arc::from(self.domain.name());
        let action: Arc<str> = Arc::from("session.open");
        if session.outbound.domain().as_str() != self.domain.name()
            || session.outbound.session() != session.context.session
        {
            return Err(ProviderError::Denied {
                domain,
                action,
                reason: Arc::from("outbound lane does not match the mapped session"),
            });
        }
        let mut state = self.state.lock();
        if state.closed {
            return Err(ProviderError::Failed {
                domain,
                action,
                reason: Arc::from("provider is closed"),
            });
        }
        if let Some(existing) = state.sessions.get(&session.context.session) {
            return if existing.principal == session.context.principal
                && existing.source_window == session.context.source_window
            {
                Ok(())
            } else {
                Err(ProviderError::Denied {
                    domain,
                    action,
                    reason: Arc::from("session id is already bound to another lane"),
                })
            };
        }
        if state.sessions.len() >= self.limits.maximum_sessions {
            return Err(ProviderError::Denied {
                domain,
                action,
                reason: Arc::from(format!(
                    "session capacity {} is full",
                    self.limits.maximum_sessions
                )),
            });
        }
        state.sessions.insert(
            session.context.session,
            NapSession {
                principal: session.context.principal,
                source_window: session.context.source_window,
                outbound: session.outbound,
                subscriptions: BTreeMap::new(),
            },
        );
        Ok(())
    }

    fn session_closed(&self, context: &ProviderSessionContext, _reason: ProviderSessionEnd) {
        close_exact_session(&self.state, context);
    }

    fn session_revoked(&self, context: &ProviderSessionContext) {
        close_exact_session(&self.state, context);
    }
}

fn normalize_error_domain(error: ProviderError, domain: NapDomain) -> ProviderError {
    let domain = Arc::from(domain.name());
    match error {
        ProviderError::InvalidPayload { action, reason, .. } => ProviderError::InvalidPayload {
            domain,
            action,
            reason,
        },
        ProviderError::Denied { action, reason, .. } => ProviderError::Denied {
            domain,
            action,
            reason,
        },
        ProviderError::Failed { action, reason, .. } => ProviderError::Failed {
            domain,
            action,
            reason,
        },
    }
}

impl Drop for NapNostrProvider {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Clone, Debug, Default)]
struct NapFilter {
    ids: Option<BTreeSet<String>>,
    authors: Option<BTreeSet<String>>,
    kinds: Option<BTreeSet<u16>>,
    tags: BTreeMap<char, BTreeSet<String>>,
    since: Option<u64>,
    until: Option<u64>,
    limit: Option<usize>,
}

#[derive(Clone, Debug)]
enum SourceChoice {
    AuthorOutboxes,
    Public,
    Pinned(BTreeSet<RelayUrl>),
}

#[derive(Debug)]
struct QueryProjection {
    rows: Vec<Row>,
    source_relays: BTreeSet<String>,
    incomplete: bool,
    /// Names the exhausted class when a host bound, rather than unresolved
    /// acquisition evidence, is what kept matching rows out of `rows`. Silent
    /// truncation is forbidden (`docs/threat-model.md`), so a projection that
    /// hit its ceiling reports the ceiling instead of reading as complete.
    bound: Option<String>,
    error: Option<String>,
}

fn validate_limits(limits: NapNostrProviderLimits) -> Result<(), ProviderError> {
    let finite = [
        limits.maximum_sessions,
        limits.maximum_subscriptions_per_session,
        limits.maximum_filters,
        limits.maximum_events,
        limits.maximum_seen_event_ids,
        limits.maximum_relays,
        limits.maximum_tags,
        limits.maximum_tag_values,
        limits.maximum_text_bytes,
        limits.maximum_response_bytes,
        limits.maximum_correlation_id_bytes,
        limits.maximum_subscription_id_bytes,
    ]
    .into_iter()
    .all(|value| value > 0)
        && limits.default_query_timeout_millis > 0
        && limits.default_query_timeout_millis <= limits.maximum_query_timeout_millis;
    if finite {
        Ok(())
    } else {
        Err(ProviderError::Failed {
            domain: Arc::from("outbox"),
            action: Arc::from("provider.build"),
            reason: Arc::from("NAP Nostr provider limits must be finite and non-zero"),
        })
    }
}

fn object_payload(request: &ProviderRequest) -> Result<&Map<String, Value>, ProviderError> {
    request
        .payload
        .as_object()
        .ok_or_else(|| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("payload must be an object"),
        })
}

fn request_domain(_request: &ProviderRequest) -> &'static str {
    "outbox"
}

fn correlation_id(
    request: &ProviderRequest,
    limits: NapNostrProviderLimits,
) -> Result<&str, ProviderError> {
    let id = request
        .correlation_id
        .as_deref()
        .ok_or_else(|| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("id is required"),
        })?;
    if id.is_empty() || id.len() > limits.maximum_correlation_id_bytes {
        return Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(format!(
                "id must be 1..={} bytes",
                limits.maximum_correlation_id_bytes
            )),
        });
    }
    Ok(id)
}

fn validate_exact_keys(
    payload: &Map<String, Value>,
    allowed: &[&str],
    request: &ProviderRequest,
) -> Result<(), ProviderError> {
    if payload.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(format!(
                "expected only these fields: {}",
                allowed.join(", ")
            )),
        });
    }
    Ok(())
}

fn required_string<'a>(
    payload: &'a Map<String, Value>,
    key: &str,
    request: &ProviderRequest,
) -> Result<&'a str, ProviderError> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(format!("{key} must be a string")),
        })
}

fn optional_object<'a>(
    payload: &'a Map<String, Value>,
    key: &str,
    request: &ProviderRequest,
) -> Result<Option<&'a Map<String, Value>>, ProviderError> {
    payload
        .get(key)
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| ProviderError::InvalidPayload {
                    domain: Arc::from(request_domain(request)),
                    action: Arc::clone(&request.action),
                    reason: Arc::from(format!("{key} must be an object")),
                })
        })
        .transpose()
}

fn validate_hex(
    value: &str,
    exact_bytes: usize,
    name: &str,
    request: &ProviderRequest,
) -> Result<(), ProviderError> {
    if value.len() == exact_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(format!(
                "{name} must be lowercase {exact_bytes}-character hex"
            )),
        })
    }
}

fn validate_sub_id(
    value: &str,
    limits: NapNostrProviderLimits,
    request: &ProviderRequest,
) -> Result<(), ProviderError> {
    if value.is_empty() || value.len() > limits.maximum_subscription_id_bytes {
        Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(format!(
                "subId must be 1..={} bytes",
                limits.maximum_subscription_id_bytes
            )),
        })
    } else {
        Ok(())
    }
}

fn parse_filters(
    value: &Value,
    limits: NapNostrProviderLimits,
    allow_single: bool,
    request: &ProviderRequest,
) -> Result<Vec<NapFilter>, ProviderError> {
    let values = if let Some(values) = value.as_array() {
        values.iter().collect::<Vec<_>>()
    } else if allow_single && value.is_object() {
        vec![value]
    } else {
        return Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("filters must be a non-empty filter array"),
        });
    };
    if values.is_empty() || values.len() > limits.maximum_filters {
        return Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(format!(
                "filters must contain 1..={} entries",
                limits.maximum_filters
            )),
        });
    }
    values
        .into_iter()
        .map(|value| parse_filter(value, limits, request))
        .collect()
}

fn parse_filter(
    value: &Value,
    limits: NapNostrProviderLimits,
    request: &ProviderRequest,
) -> Result<NapFilter, ProviderError> {
    let object = value
        .as_object()
        .ok_or_else(|| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("each filter must be an object"),
        })?;
    let mut filter = NapFilter::default();
    for (key, value) in object {
        match key.as_str() {
            "ids" => {
                let values = parse_string_array(value, limits.maximum_events, "ids", request)?;
                for id in &values {
                    validate_hex(id, 64, "filter id", request)?;
                }
                filter.ids = Some(values.into_iter().collect());
            }
            "authors" => {
                let values =
                    parse_string_array(value, limits.maximum_filters * 16, "authors", request)?;
                for author in &values {
                    validate_hex(author, 64, "filter author", request)?;
                }
                filter.authors = Some(values.into_iter().collect());
            }
            "kinds" => {
                let values = value
                    .as_array()
                    .ok_or_else(|| ProviderError::InvalidPayload {
                        domain: Arc::from(request_domain(request)),
                        action: Arc::clone(&request.action),
                        reason: Arc::from("kinds must be an array"),
                    })?;
                if values.len() > limits.maximum_filters {
                    return Err(ProviderError::InvalidPayload {
                        domain: Arc::from(request_domain(request)),
                        action: Arc::clone(&request.action),
                        reason: Arc::from("kinds exceeds the configured bound"),
                    });
                }
                filter.kinds = Some(
                    values
                        .iter()
                        .map(|value| {
                            value
                                .as_u64()
                                .and_then(|value| u16::try_from(value).ok())
                                .ok_or_else(|| ProviderError::InvalidPayload {
                                    domain: Arc::from(request_domain(request)),
                                    action: Arc::clone(&request.action),
                                    reason: Arc::from("kind must be an unsigned 16-bit integer"),
                                })
                        })
                        .collect::<Result<_, _>>()?,
                );
            }
            "since" => filter.since = Some(json_u64(value, "since", request)?),
            "until" => filter.until = Some(json_u64(value, "until", request)?),
            "limit" => {
                let limit = json_usize(value, "limit", request)?;
                if limit == 0 || limit > limits.maximum_events {
                    return Err(ProviderError::InvalidPayload {
                        domain: Arc::from(request_domain(request)),
                        action: Arc::clone(&request.action),
                        reason: Arc::from(format!(
                            "filter limit must be 1..={}",
                            limits.maximum_events
                        )),
                    });
                }
                filter.limit = Some(limit);
            }
            tag if tag.starts_with('#') && tag.chars().count() == 2 => {
                if filter.tags.len() >= limits.maximum_tags {
                    return Err(ProviderError::InvalidPayload {
                        domain: Arc::from(request_domain(request)),
                        action: Arc::clone(&request.action),
                        reason: Arc::from("filter tag-key bound exceeded"),
                    });
                }
                let name = tag.chars().nth(1).expect("two-character tag has a name");
                if name.is_control() {
                    return Err(ProviderError::InvalidPayload {
                        domain: Arc::from(request_domain(request)),
                        action: Arc::clone(&request.action),
                        reason: Arc::from("filter tag name is invalid"),
                    });
                }
                let values = parse_string_array(value, limits.maximum_tag_values, tag, request)?;
                filter.tags.insert(name, values.into_iter().collect());
            }
            _ => {
                return Err(ProviderError::InvalidPayload {
                    domain: Arc::from(request_domain(request)),
                    action: Arc::clone(&request.action),
                    reason: Arc::from(format!("unsupported filter field {key}")),
                });
            }
        }
    }
    Ok(filter)
}

fn parse_string_array(
    value: &Value,
    maximum: usize,
    name: &str,
    request: &ProviderRequest,
) -> Result<Vec<String>, ProviderError> {
    let values = value
        .as_array()
        .ok_or_else(|| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(format!("{name} must be an array")),
        })?;
    if values.len() > maximum {
        return Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(format!("{name} exceeds its {maximum}-entry bound")),
        });
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| ProviderError::InvalidPayload {
                    domain: Arc::from(request_domain(request)),
                    action: Arc::clone(&request.action),
                    reason: Arc::from(format!("{name} entries must be strings")),
                })
        })
        .collect()
}

fn json_u64(value: &Value, name: &str, request: &ProviderRequest) -> Result<u64, ProviderError> {
    value.as_u64().ok_or_else(|| ProviderError::InvalidPayload {
        domain: Arc::from(request_domain(request)),
        action: Arc::clone(&request.action),
        reason: Arc::from(format!("{name} must be a non-negative integer")),
    })
}

fn json_usize(
    value: &Value,
    name: &str,
    request: &ProviderRequest,
) -> Result<usize, ProviderError> {
    json_u64(value, name, request).and_then(|value| {
        usize::try_from(value).map_err(|_| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(format!("{name} is too large")),
        })
    })
}

fn parse_timeout(
    options: Option<&Map<String, Value>>,
    limits: NapNostrProviderLimits,
    request: &ProviderRequest,
) -> Result<Duration, ProviderError> {
    let millis = options
        .and_then(|options| options.get("timeoutMs"))
        .map(|value| json_u64(value, "options.timeoutMs", request))
        .transpose()?
        .unwrap_or(limits.default_query_timeout_millis);
    if millis == 0 || millis > limits.maximum_query_timeout_millis {
        return Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(format!(
                "options.timeoutMs must be 1..={}",
                limits.maximum_query_timeout_millis
            )),
        });
    }
    Ok(Duration::from_millis(millis))
}

fn parse_query_limit(
    options: Option<&Map<String, Value>>,
    limits: NapNostrProviderLimits,
    request: &ProviderRequest,
) -> Result<Option<usize>, ProviderError> {
    options
        .and_then(|options| options.get("limit"))
        .map(|value| {
            let limit = json_usize(value, "options.limit", request)?;
            if limit == 0 || limit > limits.maximum_events {
                return Err(ProviderError::InvalidPayload {
                    domain: Arc::from(request_domain(request)),
                    action: Arc::clone(&request.action),
                    reason: Arc::from(format!(
                        "options.limit must be 1..={}",
                        limits.maximum_events
                    )),
                });
            }
            Ok(limit)
        })
        .transpose()
}

fn parse_author_hints(
    options: Option<&Map<String, Value>>,
    limits: NapNostrProviderLimits,
    request: &ProviderRequest,
) -> Result<Vec<String>, ProviderError> {
    let Some(value) = options.and_then(|options| options.get("authors")) else {
        return Ok(Vec::new());
    };
    let authors = parse_string_array(
        value,
        limits.maximum_filters * 16,
        "options.authors",
        request,
    )?;
    for author in &authors {
        validate_hex(author, 64, "options author", request)?;
    }
    Ok(authors)
}

fn parse_relay_hints(
    options: Option<&Map<String, Value>>,
    limits: NapNostrProviderLimits,
    request: &ProviderRequest,
) -> Result<Vec<RelayUrl>, ProviderError> {
    let Some(value) = options.and_then(|options| options.get("relays")) else {
        return Ok(Vec::new());
    };
    let relays = parse_string_array(value, limits.maximum_relays, "options.relays", request)?;
    parse_relays(
        &relays.iter().map(String::as_str).collect::<Vec<_>>(),
        limits,
        request,
    )
}

fn parse_relays(
    relays: &[&str],
    limits: NapNostrProviderLimits,
    request: &ProviderRequest,
) -> Result<Vec<RelayUrl>, ProviderError> {
    if relays.is_empty() || relays.len() > limits.maximum_relays {
        return Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(format!(
                "relay set must contain 1..={} URLs",
                limits.maximum_relays
            )),
        });
    }
    relays
        .iter()
        .map(|relay| {
            RelayUrl::parse(relay).map_err(|_| ProviderError::InvalidPayload {
                domain: Arc::from(request_domain(request)),
                action: Arc::clone(&request.action),
                reason: Arc::from("relay URL is invalid"),
            })
        })
        .collect()
}

fn cap_filter_limits(mut filters: Vec<NapFilter>, requested: Option<usize>) -> Vec<NapFilter> {
    if let Some(requested) = requested {
        for filter in &mut filters {
            filter.limit = Some(filter.limit.unwrap_or(requested).min(requested));
        }
    }
    filters
}

fn read_source(
    domain: NapDomain,
    filters: &[NapFilter],
    author_hints: &[String],
    relays: Vec<RelayUrl>,
    request: &ProviderRequest,
) -> Result<SourceChoice, ProviderError> {
    if !relays.is_empty() {
        return Ok(SourceChoice::Pinned(relays.into_iter().collect()));
    }
    if domain == NapDomain::Relay {
        return Ok(SourceChoice::Public);
    }
    let bound = filters
        .iter()
        .filter(|filter| filter.authors.is_some())
        .count();
    if bound == filters.len() {
        return Ok(SourceChoice::AuthorOutboxes);
    }
    if bound == 0 && author_hints.is_empty() {
        return Ok(SourceChoice::Public);
    }
    Err(ProviderError::InvalidPayload {
        domain: Arc::from(request_domain(request)),
        action: Arc::clone(&request.action),
        reason: Arc::from(
            "outbox filters must consistently bind full authors unless explicit relay hints are supplied",
        ),
    })
}

fn broad_demand(
    filters: &[NapFilter],
    source: SourceChoice,
    request: &ProviderRequest,
) -> Result<Demand, ProviderError> {
    let all_have = |field: fn(&NapFilter) -> bool| filters.iter().all(field);
    let kinds = all_have(|filter| filter.kinds.is_some()).then(|| {
        filters
            .iter()
            .flat_map(|filter| filter.kinds.iter().flatten().copied())
            .collect()
    });
    let authors = all_have(|filter| filter.authors.is_some()).then(|| {
        Binding::Literal(
            filters
                .iter()
                .flat_map(|filter| filter.authors.iter().flatten().cloned())
                .collect(),
        )
    });
    let ids = all_have(|filter| filter.ids.is_some()).then(|| {
        Binding::Literal(
            filters
                .iter()
                .flat_map(|filter| filter.ids.iter().flatten().cloned())
                .collect(),
        )
    });
    let common_tag_names = filters
        .first()
        .map(|filter| filter.tags.keys().copied().collect::<BTreeSet<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|name| filters.iter().all(|filter| filter.tags.contains_key(name)))
        .collect::<Vec<_>>();
    let mut tags = BTreeMap::new();
    for name in common_tag_names {
        let indexed = IndexedTagName::new(name).ok_or_else(|| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("filter tag name is unsupported by NMP"),
        })?;
        tags.insert(
            indexed,
            Binding::Literal(
                filters
                    .iter()
                    .flat_map(|filter| filter.tags[&name].iter().cloned())
                    .collect(),
            ),
        );
    }
    let since = all_have(|filter| filter.since.is_some())
        .then(|| filters.iter().filter_map(|filter| filter.since).min())
        .flatten();
    let until = all_have(|filter| filter.until.is_some())
        .then(|| filters.iter().filter_map(|filter| filter.until).max())
        .flatten();
    let selection = Filter {
        kinds,
        authors,
        ids,
        tags,
        since,
        until,
        // NMP windows own the bounded row count for these observations.
        // Declaring a filter limit as well is an invalid double bound.
        limit: None,
    };
    let source = match source {
        SourceChoice::AuthorOutboxes => SourceAuthority::AuthorOutboxes,
        SourceChoice::Public => SourceAuthority::Public,
        SourceChoice::Pinned(relays) => SourceAuthority::Pinned(relays),
    };
    Demand::new(selection, source, AccessContext::Public).map_err(|error| {
        ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(error.to_string()),
        }
    })
}

fn read_once(
    engine: &Engine,
    demand: Demand,
    filters: &[NapFilter],
    timeout: Duration,
    limits: NapNostrProviderLimits,
    cancellation: &nmp_native_runtime_core::Cancellation,
) -> QueryProjection {
    if cancellation.is_cancelled() {
        return QueryProjection {
            rows: Vec::new(),
            source_relays: BTreeSet::new(),
            incomplete: true,
            bound: None,
            error: Some("request was cancelled".to_owned()),
        };
    }
    let maximum = NonZeroUsize::new(limits.maximum_events)
        .expect("validated provider event maximum is non-zero");
    let subscription = match engine.observe(
        LiveQuery(demand),
        Some(Window::Expandable {
            initial: maximum,
            max: maximum,
        }),
    ) {
        Ok(subscription) => subscription,
        Err(error) => {
            return QueryProjection {
                rows: Vec::new(),
                source_relays: BTreeSet::new(),
                incomplete: true,
                bound: None,
                error: Some(error.to_string()),
            };
        }
    };
    let deadline = Instant::now() + timeout;
    let mut latest_rows = Vec::new();
    let mut source_relays = BTreeSet::new();
    let mut incomplete = true;
    let mut bound_reached = false;
    let mut frames = 0_usize;
    loop {
        let now = Instant::now();
        if now >= deadline || cancellation.is_cancelled() {
            break;
        }
        let frame = subscription
            .recv_timeout(deadline.saturating_duration_since(now))
            .map_err(|error| error.to_string());
        let Ok(frame) = frame else {
            break;
        };
        frames += 1;
        if let Some(window) = frame.window {
            // A window delivered at its declared ceiling means canonical rows
            // older than the oldest delivered one may exist and were never
            // offered to this projection at all.
            let window_at_bound = window.rows.len() >= limits.maximum_events
                || matches!(window.load, WindowLoad::AtBound { .. });
            let selection = select_rows(window.rows, filters, limits.maximum_events);
            bound_reached = window_at_bound || selection.bound_reached;
            latest_rows = selection.rows;
        }
        source_relays = frame
            .evidence
            .sources
            .iter()
            .map(|source| source.relay.to_string())
            .collect();
        // Acquisition shortfall drives the wait; the host bound never does,
        // since no additional frame can lift a ceiling this observation was
        // opened with. Reporting still folds both in below.
        let acquisition_incomplete = projection_incomplete(&frame.evidence);
        incomplete = acquisition_incomplete || bound_reached;
        if !acquisition_incomplete || frames >= 256 {
            break;
        }
    }
    QueryProjection {
        rows: latest_rows,
        source_relays,
        incomplete,
        bound: bound_reached.then(|| {
            format!(
                "query event bound reached ({} events)",
                limits.maximum_events
            )
        }),
        error: cancellation
            .is_cancelled()
            .then(|| "request was cancelled".to_owned()),
    }
}

fn projection_incomplete(evidence: &nmp::AcquisitionEvidence) -> bool {
    !evidence.shortfall.is_empty()
        || evidence.sources.is_empty()
        || evidence
            .sources
            .iter()
            .any(|source| source.reconciled_through.is_none())
}

/// Rows chosen for one projection, plus whether the host's `maximum_events`
/// ceiling kept a matching row out. A caller-supplied `filter.limit` is the
/// napplet's own bound and never sets `bound_reached`; only the host ceiling
/// standing in for it does, because that is the cut the napplet cannot see.
#[derive(Debug)]
struct RowSelection {
    rows: Vec<Row>,
    bound_reached: bool,
}

fn select_rows(rows: Vec<Row>, filters: &[NapFilter], maximum: usize) -> RowSelection {
    let mut selected = BTreeMap::<String, Row>::new();
    let mut bound_reached = false;
    for filter in filters {
        let requested = filter.limit.unwrap_or(maximum);
        let limit = requested.min(maximum);
        let mut matching = rows.iter().filter(|row| event_matches(&row.event, filter));
        let mut taken = 0_usize;
        for row in matching.by_ref().take(limit) {
            selected.insert(row.event.id.to_string(), row.clone());
            taken += 1;
            if selected.len() >= maximum {
                break;
            }
        }
        // Whatever still matches was dropped here. Attribute the drop to the
        // host ceiling only when the ceiling — not the napplet's own limit —
        // is what stopped the take.
        if matching.next().is_some() && (taken < limit || requested >= maximum) {
            bound_reached = true;
        }
    }
    let mut rows = selected.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .event
            .created_at
            .cmp(&left.event.created_at)
            .then_with(|| left.event.id.cmp(&right.event.id))
    });
    bound_reached |= rows.len() > maximum;
    rows.truncate(maximum);
    RowSelection {
        rows,
        bound_reached,
    }
}

fn event_matches(event: &nmp::Event, filter: &NapFilter) -> bool {
    let value = match serde_json::to_value(event) {
        Ok(Value::Object(value)) => value,
        _ => return false,
    };
    let id = value.get("id").and_then(Value::as_str).unwrap_or_default();
    let author = value
        .get("pubkey")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let kind = value.get("kind").and_then(Value::as_u64);
    let created_at = value.get("created_at").and_then(Value::as_u64);
    if filter.ids.as_ref().is_some_and(|ids| !ids.contains(id))
        || filter
            .authors
            .as_ref()
            .is_some_and(|authors| !authors.contains(author))
        || filter
            .kinds
            .as_ref()
            .is_some_and(|kinds| !kind.is_some_and(|kind| kinds.contains(&(kind as u16))))
        || filter
            .since
            .is_some_and(|since| created_at.is_none_or(|created_at| created_at < since))
        || filter
            .until
            .is_some_and(|until| created_at.is_none_or(|created_at| created_at > until))
    {
        return false;
    }
    let tags = value
        .get("tags")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    filter.tags.iter().all(|(name, required)| {
        tags.iter().any(|tag| {
            tag.as_array().is_some_and(|tag| {
                tag.first().and_then(Value::as_str) == Some(name.encode_utf8(&mut [0_u8; 4]))
                    && tag
                        .get(1)
                        .and_then(Value::as_str)
                        .is_some_and(|value| required.contains(value))
            })
        })
    })
}

fn row_result(row: &Row) -> Value {
    let hints = row
        .sources
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if hints.is_empty() {
        json!({"event": row.event})
    } else {
        json!({"event": row.event, "sidecar": {"relayHints": hints}})
    }
}

fn query_result(
    domain: NapDomain,
    id: &str,
    get_event: bool,
    projection: QueryProjection,
) -> Value {
    if get_event {
        let mut value = json!({
            "type": "outbox.getEvent.result",
            "id": id,
            "incomplete": projection.incomplete,
        });
        if let Some(row) = projection.rows.first() {
            value["result"] = row_result(row);
        }
        if let Some(bound) = projection.bound {
            value["reason"] = Value::String(bound);
        }
        if let Some(error) = projection.error {
            value["error"] = Value::String(error);
        }
        value
    } else {
        let mut value = json!({
            "type": format!("{}.query.result", domain.name()),
            "id": id,
            "events": projection.rows.iter().map(row_result).collect::<Vec<_>>(),
        });
        if domain == NapDomain::Outbox {
            value["incomplete"] = Value::Bool(projection.incomplete);
        } else if projection.incomplete {
            // relay.query's pinned shell API remains an Array, so its
            // projection carries this bounded evidence as an array property.
            // Preserve rows while making a partial public window observable.
            value["incomplete"] = Value::Bool(true);
        }
        // Name the exhausted class alongside the flag, the way a bounded
        // subscription names its own: `incomplete` alone cannot tell a napplet
        // whether relays are still answering or the ceiling cut the result.
        if let Some(bound) = projection.bound {
            value["reason"] = Value::String(bound);
        }
        if let Some(error) = projection.error {
            value["error"] = Value::String(error);
        } else if domain == NapDomain::Relay && projection.incomplete && projection.rows.is_empty()
        {
            value["error"] =
                Value::String("NMP reported unresolved relay acquisition evidence".to_owned());
        }
        value
    }
}

fn drain_subscription(
    subscription: nmp::Subscription,
    domain: NapDomain,
    outbound: &ProviderPushSender,
    sub_id: &str,
    filters: &[NapFilter],
    limits: NapNostrProviderLimits,
    cancellation: &nmp_native_runtime_core::Cancellation,
) {
    let mut seen = BTreeSet::new();
    let mut eose_sent = false;
    while let Ok(frame) = subscription.recv() {
        if cancellation.is_cancelled() {
            return;
        }
        let rows = frame
            .window
            .map(|window| select_rows(window.rows, filters, limits.maximum_events).rows)
            .unwrap_or_default();
        for row in rows {
            let event_id = row.event.id.to_string();
            if seen.contains(&event_id) {
                continue;
            }
            if seen.len() >= limits.maximum_seen_event_ids {
                let mut fields = Map::new();
                fields.insert("subId".to_owned(), Value::String(sub_id.to_owned()));
                fields.insert(
                    "reason".to_owned(),
                    Value::String("subscription event-id bound reached".to_owned()),
                );
                let _ = outbound.push(domain.closed_type(), fields, Some(sub_id));
                return;
            }
            seen.insert(event_id);
            let mut fields = Map::new();
            fields.insert("subId".to_owned(), Value::String(sub_id.to_owned()));
            fields.insert("result".to_owned(), row_result(&row));
            if outbound.push(domain.event_type(), fields, None).is_err() {
                return;
            }
        }
        if domain == NapDomain::Relay && !eose_sent && !projection_incomplete(&frame.evidence) {
            let mut fields = Map::new();
            fields.insert("subId".to_owned(), Value::String(sub_id.to_owned()));
            if outbound.push("relay.eose", fields, Some(sub_id)).is_err() {
                return;
            }
            eose_sent = true;
        }
    }
    if !cancellation.is_cancelled() {
        let mut fields = Map::new();
        fields.insert("subId".to_owned(), Value::String(sub_id.to_owned()));
        fields.insert(
            "reason".to_owned(),
            Value::String("NMP observation closed".to_owned()),
        );
        let _ = outbound.push(domain.closed_type(), fields, Some(sub_id));
    }
}

fn remove_finished_subscription(
    state: Weak<Mutex<ProviderState>>,
    session: SessionId,
    principal: &Principal,
    sub_id: &str,
) {
    let Some(state) = state.upgrade() else {
        return;
    };
    let mut state = state.lock();
    if let Some(session) = state.sessions.get_mut(&session)
        && &session.principal == principal
    {
        session.subscriptions.remove(sub_id);
    }
}

fn close_exact_session(state: &Arc<Mutex<ProviderState>>, context: &ProviderSessionContext) {
    let subscriptions = {
        let mut state = state.lock();
        let matches = state.sessions.get(&context.session).is_some_and(|session| {
            session.principal == context.principal && session.source_window == context.source_window
        });
        if !matches {
            return;
        }
        state
            .sessions
            .remove(&context.session)
            .into_iter()
            .flat_map(|session| session.subscriptions.into_values())
            .flat_map(|subscription| subscription.cancels)
            .collect::<Vec<_>>()
    };
    for cancel in subscriptions {
        cancel.cancel();
    }
}

fn parse_event_template(
    value: &Value,
    account: &AccountRef,
    limits: NapNostrProviderLimits,
    request: &ProviderRequest,
) -> Result<UnsignedEvent, ProviderError> {
    let object = value
        .as_object()
        .ok_or_else(|| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("event must be an object"),
        })?;
    if object.len() != 4
        || object
            .keys()
            .any(|key| !["kind", "content", "tags", "created_at"].contains(&key.as_str()))
    {
        return Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("event template requires exactly kind, content, tags, created_at"),
        });
    }
    let kind = object
        .get("kind")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("event.kind must be an unsigned 16-bit integer"),
        })?;
    let content = object
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("event.content must be a string"),
        })?;
    if content.len() > limits.maximum_text_bytes {
        return Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("event.content exceeds the configured byte bound"),
        });
    }
    let created_at = object
        .get("created_at")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("event.created_at must be a non-negative integer"),
        })?;
    let tags = object
        .get("tags")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("event.tags must be an array"),
        })?;
    if tags.len() > limits.maximum_tags {
        return Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("event.tags exceeds the configured bound"),
        });
    }
    let tags = tags
        .iter()
        .map(|tag| {
            let values = parse_string_array(tag, limits.maximum_tag_values, "event tag", request)?;
            if values.is_empty()
                || values
                    .iter()
                    .any(|value| value.len() > limits.maximum_text_bytes)
            {
                return Err(ProviderError::InvalidPayload {
                    domain: Arc::from(request_domain(request)),
                    action: Arc::clone(&request.action),
                    reason: Arc::from("event tag is empty or exceeds the configured byte bound"),
                });
            }
            nmp::Tag::parse(values).map_err(|error| ProviderError::InvalidPayload {
                domain: Arc::from(request_domain(request)),
                action: Arc::clone(&request.action),
                reason: Arc::from(format!("invalid event tag: {error}")),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let author = PublicKey::from_str(&account.0).map_err(|_| ProviderError::Failed {
        domain: Arc::from(request_domain(request)),
        action: Arc::clone(&request.action),
        reason: Arc::from("frozen account is invalid"),
    })?;
    Ok(UnsignedEvent::new(
        author,
        nmp::Timestamp::from(created_at),
        nmp::Kind::from(kind),
        tags,
        content.to_owned(),
    ))
}

fn validate_publish_options(
    options: Option<&Map<String, Value>>,
    limits: NapNostrProviderLimits,
    request: &ProviderRequest,
) -> Result<(), ProviderError> {
    let Some(options) = options else {
        return Ok(());
    };
    if options
        .keys()
        .any(|key| !["relays", "toOutbox", "toInboxes"].contains(&key.as_str()))
    {
        return Err(ProviderError::InvalidPayload {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from("publish options contain unknown fields"),
        });
    }
    if options
        .get("toOutbox")
        .is_some_and(|value| value != &Value::Bool(true))
        || options.get("relays").is_some()
        || options.get("toInboxes").is_some()
    {
        return Err(ProviderError::Denied {
            domain: Arc::from(request_domain(request)),
            action: Arc::clone(&request.action),
            reason: Arc::from(
                "the pinned app-tier NMP facade currently supports NAP publish through the author outbox route only",
            ),
        });
    }
    let _ = limits;
    Ok(())
}

struct NapPublishCompletion {
    domain: NapDomain,
    id: Arc<str>,
    outbound: ProviderPushSender,
    engine: Arc<Engine>,
    maximum_response_bytes: usize,
}

impl fmt::Debug for NapPublishCompletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NapPublishCompletion")
            .field("domain", &self.domain)
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl ProviderWriteCompletion for NapPublishCompletion {
    fn into_receipt_sink(self: Box<Self>) -> Arc<dyn ReceiptEventSink> {
        Arc::new(NapPublishReceiptSink {
            domain: self.domain,
            id: self.id,
            outbound: self.outbound,
            engine: self.engine,
            maximum_response_bytes: self.maximum_response_bytes,
            delivered: AtomicBool::new(false),
        })
    }

    fn refused(self: Box<Self>, reason: Arc<str>) {
        let sink = self.into_receipt_sink();
        sink.close(Some(reason));
    }
}

struct NapPublishReceiptSink {
    domain: NapDomain,
    id: Arc<str>,
    outbound: ProviderPushSender,
    engine: Arc<Engine>,
    maximum_response_bytes: usize,
    delivered: AtomicBool,
}

impl fmt::Debug for NapPublishReceiptSink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NapPublishReceiptSink")
            .field("domain", &self.domain)
            .field("id", &self.id)
            .field("delivered", &self.delivered.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl ReceiptEventSink for NapPublishReceiptSink {
    fn push_latest(&self, snapshot: ReceiptSnapshot) -> Result<(), ReceiptSinkError> {
        if self.delivered.load(Ordering::Acquire) {
            return Ok(());
        }
        let value = snapshot
            .state
            .decode()
            .map_err(|_| ReceiptSinkError::FrameTooLarge)?;
        let state = value
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let terminal = matches!(
            state,
            "delivered"
                | "partial_delivery"
                | "exhausted"
                | "failed"
                | "cancelled"
                | "replaceable_conflict"
        );
        if !terminal {
            return Ok(());
        }
        if self.delivered.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let event_id = value
            .get("eventId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let relays = value
            .get("relays")
            .and_then(Value::as_object)
            .map(|relays| {
                relays
                    .iter()
                    .map(|(relay, result)| {
                        (
                            relay.clone(),
                            Value::Bool(
                                result.get("state").and_then(Value::as_str) == Some("acked"),
                            ),
                        )
                    })
                    .collect::<Map<_, _>>()
            })
            .unwrap_or_default();
        let mut ok = relays.values().any(|value| value == &Value::Bool(true));
        let mut response = json!({
            "type": format!("{}.publish.result", self.domain.name()),
            "id": self.id,
            "receiptId": snapshot.receipt_id.0,
            "ok": ok,
            "relays": relays,
        });
        if let Some(event_id) = &event_id {
            response["eventId"] = Value::String(event_id.clone());
            if let Some(event) = cached_event_by_id(&self.engine, event_id) {
                response["event"] = serde_json::to_value(event).unwrap_or(Value::Null);
            } else if self.domain == NapDomain::Relay && ok {
                ok = false;
                response["ok"] = Value::Bool(false);
                response["error"] = Value::String(
                    "signed event was not readable from NMP canonical state".to_owned(),
                );
            }
        }
        if !ok && response.get("error").is_none() {
            response["error"] = Value::String(
                value
                    .get("failure")
                    .and_then(Value::as_str)
                    .unwrap_or("NMP delivery did not receive a relay acknowledgement")
                    .to_owned(),
            );
        }
        push_value(
            &self.outbound,
            response,
            self.maximum_response_bytes,
            Some(&self.id),
        )
        .map(|_| ())
        .map_err(|_| ReceiptSinkError::Closed)
    }

    fn close(&self, reason: Option<Arc<str>>) {
        if self.delivered.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = push_value(
            &self.outbound,
            json!({
                "type": format!("{}.publish.result", self.domain.name()),
                "id": self.id,
                "ok": false,
                "error": reason.as_deref().unwrap_or("NMP receipt observation closed"),
            }),
            self.maximum_response_bytes,
            Some(&self.id),
        );
    }
}

fn cached_event_by_id(engine: &Engine, event_id: &str) -> Option<nmp::Event> {
    let mut demand = Demand::from_filter(Filter {
        ids: Some(Binding::Literal(BTreeSet::from([event_id.to_owned()]))),
        // The finite window below is the sole row bound. The pinned NMP
        // facade rejects a selection that also declares Filter.limit.
        limit: None,
        ..Filter::default()
    });
    demand.cache = CacheMode::Agnostic;
    demand.freshness = Freshness::CacheOnly;
    let one = NonZeroUsize::new(1).expect("one is non-zero");
    let subscription = engine
        .observe(
            LiveQuery(demand),
            Some(Window::Expandable {
                initial: one,
                max: one,
            }),
        )
        .ok()?;
    subscription
        .recv_timeout(RECEIPT_EVENT_LOOKUP_TIMEOUT)
        .ok()?
        .window?
        .rows
        .into_iter()
        .next()
        .map(|row| row.event)
}

fn resolve_result(
    id: &str,
    authors: &[String],
    _direction: &str,
    projection: QueryProjection,
) -> Value {
    let relays = projection.source_relays;
    let missing = relays.is_empty().then(|| authors.to_vec());
    let mut value = json!({
        "type": "outbox.resolveRelays.result",
        "id": id,
        "plan": {
            "relays": relays,
            "source": if missing.is_some() { "fallback" } else { "policy" },
        },
    });
    if let Some(missing) = missing {
        value["plan"]["missingAuthors"] = serde_json::to_value(missing).unwrap_or(Value::Null);
    }
    if projection.incomplete {
        value["error"] = Value::String(
            projection
                .error
                .unwrap_or_else(|| "relay-list acquisition evidence is unresolved".to_owned()),
        );
    }
    value
}

fn push_value(
    outbound: &ProviderPushSender,
    value: Value,
    maximum_bytes: usize,
    conflation_key: Option<&str>,
) -> Result<u64, nmp_native_nap_bridge::ProviderPushError> {
    let envelope = BoundedJson::from_value(&value, maximum_bytes).map_err(|error| {
        nmp_native_nap_bridge::ProviderPushError::Malformed(Arc::from(error.to_string()))
    })?;
    outbound.push_envelope(&envelope, conflation_key)
}

#[cfg(test)]
mod tests {
    use nmp::{
        Durability, EngineConfig, ReceiptReattachment, WriteIntent, WritePayload, WriteRouting,
        WriteStatus,
    };
    use nmp_native_nap_bridge::{
        BridgeLimits, DispatchOutcome, InjectionPlan, MemoryActivitySink, ProviderPushObserver,
        ProviderRegistry, SessionContext, SourceWindowId,
    };
    use nmp_native_runtime_core::{
        ExecutionProfile, GrantDecision, GrantLedger, GrantLimits, HostDataPlane, ResourceLimits,
        ResourceTracker, Sensitivity, WriteReceiptId,
    };

    use super::*;

    struct NapRig {
        plane: Arc<NmpDataPlane>,
        outbox: Arc<NapNostrProvider>,
        relay: Arc<NapNostrProvider>,
        registry: ProviderRegistry,
        context: SessionContext,
        plan: InjectionPlan,
        observer: ProviderPushObserver,
    }

    fn principal() -> Principal {
        Principal::new("a".repeat(64), "good-morning", "b".repeat(64)).unwrap()
    }

    fn nap_rig() -> NapRig {
        nap_rig_with_config(EngineConfig::default())
    }

    fn nap_rig_with_config(config: EngineConfig) -> NapRig {
        let plane = Arc::new(NmpDataPlane::open(config, 8).unwrap());
        let providers =
            NapNostrProviderSet::new(Arc::clone(&plane), NapNostrProviderLimits::default())
                .unwrap();
        let resources = Arc::new(ResourceTracker::new(ResourceLimits::default()).unwrap());
        let grants =
            Arc::new(GrantLedger::new(GrantLimits::default(), Arc::clone(&resources)).unwrap());
        let mut registry = ProviderRegistry::new(
            BridgeLimits::default(),
            resources,
            Arc::clone(&grants),
            Arc::new(MemoryActivitySink::bounded(32)),
        )
        .unwrap();
        let domains = BTreeSet::from([
            Capability::new(OUTBOX_DOMAIN).unwrap(),
            Capability::new(RELAY_DOMAIN).unwrap(),
        ]);
        for domain in &domains {
            grants
                .set(
                    principal(),
                    domain.clone(),
                    Sensitivity::Sensitive,
                    GrantDecision::AllowExactBuild,
                )
                .unwrap();
        }
        let outbox_provider = Arc::clone(&providers.outbox);
        let relay_provider = Arc::clone(&providers.relay);
        let outbox: Arc<dyn Provider> = providers.outbox;
        let relay: Arc<dyn Provider> = providers.relay;
        registry.register(outbox).unwrap();
        registry.register(relay).unwrap();
        let context = SessionContext {
            id: SessionId(7),
            principal: principal(),
            profile: ExecutionProfile::Legacy,
        };
        let plan = registry
            .negotiate(&context.principal, context.profile, &domains)
            .unwrap();
        let observer = registry
            .open_session_bound(&context, &plan, SourceWindowId(19), 0)
            .unwrap();
        registry.mark_session_ready(context.id).unwrap();
        NapRig {
            plane,
            outbox: outbox_provider,
            relay: relay_provider,
            registry,
            context,
            plan,
            observer,
        }
    }

    fn dispatch(rig: &NapRig, envelope: Value) -> DispatchOutcome {
        rig.registry
            .dispatch(
                &rig.context,
                &rig.plan,
                serde_json::to_vec(&envelope).unwrap().as_slice(),
                0,
            )
            .unwrap()
    }

    async fn pushed(rig: &mut NapRig, outcome: DispatchOutcome) -> Value {
        let DispatchOutcome::Handled(call) = outcome else {
            panic!("NAP request must be handled");
        };
        assert!(call.response.is_none());
        assert!(!call.is_active());
        let batch = tokio::time::timeout(Duration::from_secs(3), rig.observer.changed(8))
            .await
            .expect("provider push must remain within its bounded deadline")
            .unwrap();
        assert_eq!(batch.pushes.len(), 1);
        batch.pushes[0].envelope.decode().unwrap()
    }

    fn public_note() -> nmp::Event {
        serde_json::from_value(json!({
            "kind": 1,
            "id": "134ce22e517d5c5cd574fe276e52cf713d7ca1228da7530cef10c58286c03025",
            "pubkey": "974ab003f85a1c8d6da5ed68f215a4a7b5d1c8b5382013a93fd301abf97a68d4",
            "created_at": 1_700_000_000_u64,
            "tags": [],
            "content": "deterministic public note",
            "sig": "ef6e98957a40ae0eba44499a34b30cec9dcf0c4e933de9a753e9b0b7e48eccfce56edc5b7704380dc4d2618876756a198f1acbf3e273a301881d1b702e19e15d",
        }))
        .unwrap()
    }

    fn seed_canonical_event(plane: &NmpDataPlane, event: &nmp::Event) {
        let statuses = plane
            .engine
            .publish(WriteIntent {
                payload: WritePayload::Signed(event.clone()),
                durability: Durability::Durable,
                routing: WriteRouting::AuthorOutbox,
                identity_override: None,
                correlation: None,
            })
            .unwrap();
        for _ in 0..32 {
            match statuses.recv_timeout(Duration::from_secs(2)).unwrap() {
                WriteStatus::Signed(id) if id == event.id => return,
                WriteStatus::Failed(reason) => panic!("signed fixture was rejected: {reason}"),
                _ => {}
            }
        }
        panic!("signed fixture never reached canonical NMP state");
    }

    #[test]
    fn receipt_event_lookup_reads_the_canonical_nmp_row() {
        let plane = NmpDataPlane::open(EngineConfig::default(), 2).unwrap();
        let event = public_note();
        seed_canonical_event(&plane, &event);

        let resolved = cached_event_by_id(&plane.engine, &event.id.to_string())
            .expect("a signed receipt event must remain readable from canonical NMP state");
        assert_eq!(resolved.id, event.id);

        plane.close();
    }

    fn request(action: &str, payload: Value) -> ProviderRequest {
        let resources = nmp_native_runtime_core::ResourceTracker::new(
            nmp_native_runtime_core::ResourceLimits::default(),
        )
        .unwrap();
        let principal = Principal::new("a".repeat(64), "fixture", "b".repeat(64)).unwrap();
        let work = resources
            .admit(
                SessionId(1),
                Some(Capability::new(OUTBOX_DOMAIN).unwrap()),
                nmp_native_runtime_core::ResourceClass::ProviderCall,
            )
            .unwrap();
        ProviderRequest {
            principal,
            session: SessionId(1),
            action: Arc::from(action),
            correlation_id: Some(Arc::from("request-1")),
            payload,
            work,
        }
    }

    #[test]
    fn broad_filter_coalesces_good_morning_profile_queries_without_widening_source() {
        let filters = parse_filters(
            &json!([
                {"kinds": [0], "authors": ["a".repeat(64)], "limit": 1},
                {"kinds": [0], "authors": ["b".repeat(64)], "limit": 1}
            ]),
            NapNostrProviderLimits::default(),
            true,
            &request("query", json!({})),
        )
        .unwrap();
        let demand = broad_demand(
            &filters,
            SourceChoice::AuthorOutboxes,
            &request("query", json!({})),
        )
        .unwrap();
        assert_eq!(demand.selection.kinds, Some(BTreeSet::from([0])));
        assert!(matches!(demand.source, SourceAuthority::AuthorOutboxes));
        let Binding::Literal(authors) = demand.selection.authors.unwrap() else {
            panic!("authors must remain literal");
        };
        assert_eq!(authors.len(), 2);
    }

    #[test]
    fn mixed_outbox_authority_is_refused_instead_of_silently_using_public_routing() {
        let filters = vec![
            NapFilter {
                authors: Some(BTreeSet::from(["a".repeat(64)])),
                ..NapFilter::default()
            },
            NapFilter::default(),
        ];
        assert!(
            read_source(
                NapDomain::Outbox,
                &filters,
                &[],
                Vec::new(),
                &request("query", json!({})),
            )
            .is_err()
        );
    }

    #[test]
    fn publish_options_refuse_unrepresentable_route_unions() {
        assert!(
            validate_publish_options(
                json!({"toInboxes": ["a".repeat(64)]}).as_object(),
                NapNostrProviderLimits::default(),
                &request("publish", json!({})),
            )
            .is_err()
        );
    }

    #[test]
    fn query_result_keeps_partial_evidence_explicit() {
        let value = query_result(
            NapDomain::Outbox,
            "q",
            false,
            QueryProjection {
                rows: Vec::new(),
                source_relays: BTreeSet::new(),
                incomplete: true,
                bound: None,
                error: None,
            },
        );
        assert_eq!(value["incomplete"], true);
        assert!(value.get("synced").is_none());
        assert!(value.get("complete").is_none());
        assert!(value.get("reason").is_none());
    }

    fn note_row(index: u8, created_at: u64) -> Row {
        let mut value = serde_json::to_value(public_note()).unwrap();
        value["id"] = json!(format!("{index:02x}{}", "0".repeat(62)));
        value["created_at"] = json!(created_at);
        Row {
            event: serde_json::from_value(value).unwrap(),
            sources: BTreeSet::new(),
        }
    }

    #[test]
    fn select_rows_reports_the_host_bound_that_dropped_matching_rows() {
        let rows = vec![
            note_row(1, 1_700_000_003),
            note_row(2, 1_700_000_002),
            note_row(3, 1_700_000_001),
        ];

        let selection = select_rows(rows, &[NapFilter::default()], 2);

        assert_eq!(selection.rows.len(), 2);
        assert!(
            selection.bound_reached,
            "a projection cut by maximum_events must not read as complete"
        );
    }

    #[test]
    fn select_rows_keeps_a_caller_supplied_limit_off_the_host_bound() {
        let rows = vec![
            note_row(1, 1_700_000_003),
            note_row(2, 1_700_000_002),
            note_row(3, 1_700_000_001),
        ];
        let filter = NapFilter {
            limit: Some(2),
            ..NapFilter::default()
        };

        let selection = select_rows(rows, &[filter], 8);

        assert_eq!(selection.rows.len(), 2);
        assert!(
            !selection.bound_reached,
            "the napplet's own limit is not a bound it needs reported back"
        );
    }

    #[test]
    fn select_rows_reports_no_bound_when_every_matching_row_fits() {
        let rows = vec![note_row(1, 1_700_000_002), note_row(2, 1_700_000_001)];

        let selection = select_rows(rows, &[NapFilter::default()], 8);

        assert_eq!(selection.rows.len(), 2);
        assert!(!selection.bound_reached);
    }

    #[test]
    fn query_result_names_the_event_bound_that_cut_its_rows() {
        let value = query_result(
            NapDomain::Relay,
            "relay-bounded-1",
            false,
            QueryProjection {
                rows: vec![Row {
                    event: public_note(),
                    sources: BTreeSet::new(),
                }],
                source_relays: BTreeSet::from(["wss://relay.example".to_owned()]),
                incomplete: true,
                bound: Some("query event bound reached (1024 events)".to_owned()),
                error: None,
            },
        );

        assert_eq!(value["incomplete"], true);
        assert_eq!(value["reason"], "query event bound reached (1024 events)");
        assert!(value.get("error").is_none());
    }

    #[test]
    fn relay_query_projects_partial_public_evidence_even_with_rows() {
        let value = query_result(
            NapDomain::Relay,
            "relay-partial-1",
            false,
            QueryProjection {
                rows: vec![Row {
                    event: public_note(),
                    sources: BTreeSet::from(["wss://relay.example".parse().unwrap()]),
                }],
                source_relays: BTreeSet::from(["wss://relay.example".to_owned()]),
                incomplete: true,
                bound: None,
                error: None,
            },
        );
        assert_eq!(
            value["events"][0]["event"]["content"],
            "deterministic public note"
        );
        assert_eq!(value["incomplete"], true);
        assert!(value.get("error").is_none());
    }

    #[test]
    fn relay_resolution_projects_only_nmp_planned_sources() {
        let value = resolve_result(
            "resolve-1",
            &["a".repeat(64)],
            "read",
            QueryProjection {
                rows: Vec::new(),
                source_relays: BTreeSet::from([
                    "wss://one.example".to_owned(),
                    "wss://two.example".to_owned(),
                ]),
                incomplete: false,
                bound: None,
                error: None,
            },
        );

        assert_eq!(value["plan"]["source"], "policy");
        assert_eq!(
            value["plan"]["relays"],
            json!(["wss://one.example", "wss://two.example"])
        );
        assert!(value["plan"].get("missingAuthors").is_none());
    }

    #[tokio::test]
    async fn public_relay_and_good_morning_outbox_queries_read_nmp_canonical_rows() {
        let mut rig = nap_rig();
        let event = public_note();
        seed_canonical_event(&rig.plane, &event);

        let outcome = dispatch(
            &rig,
            json!({
                "type": "relay.query",
                "id": "public-query-1",
                "filters": [{"ids": [event.id.to_string()], "limit": 1}],
            }),
        );
        let relay_result = pushed(&mut rig, outcome).await;
        assert_eq!(relay_result["type"], "relay.query.result");
        assert_eq!(relay_result["id"], "public-query-1");
        assert_eq!(
            relay_result["events"][0]["event"]["id"],
            event.id.to_string()
        );
        assert!(relay_result.get("synced").is_none());
        assert!(relay_result.get("complete").is_none());

        let author = event.pubkey.to_string();
        let outcome = dispatch(
            &rig,
            json!({
                "type": "outbox.query",
                "id": "good-morning-query-1",
                "filters": [{
                    "kinds": [1],
                    "authors": [author],
                    "since": 0,
                    "limit": 1
                }],
                "options": {
                    "authors": [event.pubkey.to_string()],
                    "timeoutMs": 50
                }
            }),
        );
        let outbox_result = pushed(&mut rig, outcome).await;
        assert_eq!(outbox_result["type"], "outbox.query.result");
        assert_eq!(outbox_result["id"], "good-morning-query-1");
        assert_eq!(
            outbox_result["events"][0]["event"]["id"],
            event.id.to_string()
        );
        assert!(outbox_result.get("synced").is_none());
        assert!(outbox_result.get("complete").is_none());

        rig.registry.close_session(rig.context.id);
        rig.plane.close();
    }

    #[tokio::test]
    async fn publish_result_preserves_distinct_per_relay_receipt_outcomes() {
        let mut rig = nap_rig();
        let outbound = rig
            .outbox
            .state
            .lock()
            .sessions
            .get(&rig.context.id)
            .unwrap()
            .outbound
            .clone();
        let sink = NapPublishReceiptSink {
            domain: NapDomain::Outbox,
            id: Arc::from("publish-mixed-1"),
            outbound,
            engine: Arc::clone(&rig.plane.engine),
            maximum_response_bytes: NapNostrProviderLimits::default().maximum_response_bytes,
            delivered: AtomicBool::new(false),
        };
        sink.push_latest(ReceiptSnapshot {
            receipt_id: WriteReceiptId(Arc::from("receipt-mixed-1")),
            state: BoundedJson::from_value(
                &json!({
                    "state": "partial_delivery",
                    "relays": {
                        "wss://acked.example": {"state": "acked"},
                        "wss://rejected.example": {
                            "state": "rejected",
                            "reason": "policy"
                        }
                    }
                }),
                16 * 1024,
            )
            .unwrap(),
        })
        .unwrap();

        let batch = tokio::time::timeout(Duration::from_secs(1), rig.observer.changed(1))
            .await
            .expect("mixed receipt projection must remain bounded")
            .unwrap();
        let value = batch.pushes[0].envelope.decode().unwrap();
        assert_eq!(value["type"], "outbox.publish.result");
        assert_eq!(value["id"], "publish-mixed-1");
        assert_eq!(value["receiptId"], "receipt-mixed-1");
        assert_eq!(value["ok"], true);
        assert_eq!(value["relays"]["wss://acked.example"], true);
        assert_eq!(value["relays"]["wss://rejected.example"], false);

        rig.registry.close_session(rig.context.id);
        rig.plane.close();
    }

    #[tokio::test]
    #[ignore = "requires explicit public relay access and posts a disposable kind-1 event"]
    async fn live_outbox_publish_reaches_a_public_relay() {
        let relays = std::env::var("NMP_LIVE_TEST_RELAYS")
            .expect("set NMP_LIVE_TEST_RELAYS to a comma-separated operator relay set")
            .split(',')
            .map(str::trim)
            .filter(|relay| !relay.is_empty())
            .take(3)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        assert!(!relays.is_empty(), "at least one live relay is required");

        let mut rig = nap_rig_with_config(EngineConfig {
            indexer_relays: relays.clone(),
            app_relays: relays.clone(),
            fallback_relays: relays,
            ..EngineConfig::default()
        });
        let account = rig
            .plane
            .register_local_account(&format!("{:064x}", 21_u8))
            .expect("the disposable signer must register through NMP");
        rig.plane
            .activate_local_account(&account)
            .expect("the disposable signer must become active");
        let relay_list_filter = Filter {
            kinds: Some(BTreeSet::from([10_002])),
            authors: Some(Binding::Literal(BTreeSet::from([account
                .account
                .0
                .to_string()]))),
            ..Filter::default()
        };
        let one = NonZeroUsize::new(1).expect("one is non-zero");
        let relay_list = rig
            .plane
            .engine
            .observe(
                LiveQuery::from_filter(relay_list_filter),
                Some(Window::Expandable {
                    initial: one,
                    max: one,
                }),
            )
            .expect("NMP must open disposable-account relay discovery");
        let mut discovered_write_relays = false;
        for _ in 0..8 {
            let frame = relay_list
                .recv_timeout(Duration::from_secs(6))
                .expect("relay discovery must advance within its bounded deadline");
            if frame
                .window
                .as_ref()
                .is_some_and(|window| !window.rows.is_empty())
            {
                discovered_write_relays = true;
                break;
            }
        }
        assert!(
            discovered_write_relays,
            "NMP did not ingest the disposable account's NIP-65 relay list"
        );
        relay_list.cancel();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the wall clock must be after the Unix epoch")
            .as_secs();
        let correlation = format!("nampplets-live-outbox-{now}-{}", std::process::id());
        let outcome = dispatch(
            &rig,
            json!({
                "type": "outbox.publish",
                "id": correlation,
                "event": {
                    "kind": 1,
                    "content": format!("nampplets NAP-OUTBOX live verification {now}"),
                    "tags": [["t", "nampplets-runtime-verification"]],
                    "created_at": now
                }
            }),
        );
        let DispatchOutcome::Handled(mut call) = outcome else {
            panic!("the NAP-OUTBOX publish must be handled");
        };
        assert!(call.response.is_none());
        let proposal = call
            .take_write_proposal()
            .expect("live publish must await exact native approval");
        let (write, completion, work) = proposal.into_parts();
        let accepted = rig
            .plane
            .accept_write(write, completion.into_receipt_sink())
            .expect("native approval must transfer the write to NMP");
        drop(work);
        assert!(!accepted.receipt_id.0.is_empty());
        assert!(!call.is_active());

        let batch = tokio::time::timeout(Duration::from_secs(45), rig.observer.changed(8))
            .await
            .expect("the live NMP receipt must terminate within 45 seconds")
            .expect("the provider push lane must remain open");
        let result = batch
            .pushes
            .iter()
            .map(|push| push.envelope.decode().expect("valid provider envelope"))
            .find(|value| value["type"] == "outbox.publish.result")
            .expect("the terminal NAP-OUTBOX result must be pushed");
        assert_eq!(result["ok"], true, "live publish failed: {result}");
        let event_id = result["eventId"]
            .as_str()
            .expect("an acknowledged publish must expose its canonical event id");
        println!("NMP_LIVE_OUTBOX_EVENT_ID={event_id}");

        let relay_query = dispatch(
            &rig,
            json!({
                "type": "relay.query",
                "id": format!("{correlation}-relay-query"),
                "filters": [{"ids": [event_id]}]
            }),
        );
        let DispatchOutcome::Handled(relay_query) = relay_query else {
            panic!("the live NAP-RELAY query must be handled");
        };
        assert!(relay_query.response.is_none());
        let query_batch = tokio::time::timeout(Duration::from_secs(45), rig.observer.changed(8))
            .await
            .expect("the live NMP relay query must terminate within 45 seconds")
            .expect("the provider push lane must remain open");
        let query_result = query_batch
            .pushes
            .iter()
            .map(|push| push.envelope.decode().expect("valid provider envelope"))
            .find(|value| value["type"] == "relay.query.result")
            .expect("the live NAP-RELAY query result must be pushed");
        assert!(
            query_result.get("error").is_none(),
            "live relay query failed: {query_result}"
        );
        assert_eq!(query_result["events"][0]["event"]["id"], event_id);

        rig.registry.close_session(rig.context.id);
        rig.plane.close();
    }

    #[tokio::test]
    async fn relay_publish_ack_projects_the_canonical_signed_event() {
        let mut rig = nap_rig();
        let event = public_note();
        seed_canonical_event(&rig.plane, &event);
        let outbound = rig
            .relay
            .state
            .lock()
            .sessions
            .get(&rig.context.id)
            .unwrap()
            .outbound
            .clone();
        let sink = NapPublishReceiptSink {
            domain: NapDomain::Relay,
            id: Arc::from("relay-publish-1"),
            outbound,
            engine: Arc::clone(&rig.plane.engine),
            maximum_response_bytes: NapNostrProviderLimits::default().maximum_response_bytes,
            delivered: AtomicBool::new(false),
        };
        sink.push_latest(ReceiptSnapshot {
            receipt_id: WriteReceiptId(Arc::from("receipt-relay-1")),
            state: BoundedJson::from_value(
                &json!({
                    "state": "delivered",
                    "eventId": event.id.to_string(),
                    "relays": {
                        "wss://acked.example": {"state": "acked"}
                    }
                }),
                16 * 1024,
            )
            .unwrap(),
        })
        .unwrap();

        let batch = tokio::time::timeout(Duration::from_secs(1), rig.observer.changed(1))
            .await
            .expect("relay receipt projection must remain bounded")
            .unwrap();
        let value = batch.pushes[0].envelope.decode().unwrap();
        assert_eq!(value["type"], "relay.publish.result");
        assert_eq!(value["id"], "relay-publish-1");
        assert_eq!(value["receiptId"], "receipt-relay-1");
        assert_eq!(value["ok"], true);
        assert_eq!(value["eventId"], event.id.to_string());
        assert_eq!(value["event"]["id"], event.id.to_string());
        assert_eq!(value["relays"]["wss://acked.example"], true);

        rig.registry.close_session(rig.context.id);
        rig.plane.close();
    }

    #[tokio::test]
    async fn signed_publish_is_proposed_exactly_and_refusal_completes_the_nap_request() {
        let mut rig = nap_rig();
        let event = public_note();
        let DispatchOutcome::Handled(mut call) = dispatch(
            &rig,
            json!({
                "type": "relay.publish",
                "id": "signed-publish-1",
                "event": event,
            }),
        ) else {
            panic!("signed relay publish must be handled");
        };
        let proposal = call
            .take_write_proposal()
            .expect("signed publish must await exact native approval");
        let write = proposal.write.as_ref().unwrap();
        assert_eq!(write.origin_principal, rig.context.principal);
        assert_eq!(write.origin_session, rig.context.id);
        assert_eq!(write.account.0.as_ref(), event.pubkey.to_string());
        assert_eq!(write.approval_id.as_ref(), event.id.to_string());
        assert_eq!(write.draft.decode().unwrap()["sig"], event.sig.to_string());
        assert!(matches!(
            rig.plane
                .engine
                .reattach_by_correlation(write.approval_id.to_string())
                .unwrap(),
            ReceiptReattachment::NotFound
        ));

        proposal.refuse(Arc::from("native approval refused"));
        let batch = tokio::time::timeout(Duration::from_secs(1), rig.observer.changed(1))
            .await
            .expect("refusal result must remain bounded")
            .unwrap();
        let value = batch.pushes[0].envelope.decode().unwrap();
        assert_eq!(value["type"], "relay.publish.result");
        assert_eq!(value["id"], "signed-publish-1");
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"], "native approval refused");

        rig.registry.close_session(rig.context.id);
        rig.plane.close();
    }

    #[test]
    fn quick_gm_proposal_is_not_accepted_until_native_approval() {
        let rig = nap_rig();
        let first = rig
            .plane
            .register_local_account(&format!("{:064x}", 11_u8))
            .unwrap();
        let second = rig
            .plane
            .register_local_account(&format!("{:064x}", 12_u8))
            .unwrap();
        rig.plane.activate_local_account(&first).unwrap();

        let DispatchOutcome::Handled(mut call) = dispatch(
            &rig,
            json!({
                "type": "outbox.publish",
                "id": "quick-gm-1",
                "event": {
                    "kind": 1,
                    "content": "GM",
                    "tags": [],
                    "created_at": 1
                }
            }),
        ) else {
            panic!("Quick GM publish must be handled");
        };
        assert!(call.response.is_none());
        assert!(call.is_active());
        let proposal = call
            .take_write_proposal()
            .expect("Quick GM must produce an exact write proposal");
        assert!(!call.is_active());
        let proposed_write = proposal.write.as_ref().unwrap();
        assert_eq!(proposed_write.origin_principal, rig.context.principal);
        assert_eq!(proposed_write.origin_session, rig.context.id);
        assert_eq!(proposed_write.account, first.account);
        let draft = proposed_write.draft.decode().unwrap();
        assert_eq!(draft["content"], "GM");
        assert_eq!(draft["pubkey"], first.account.0.as_ref());
        assert_eq!(proposed_write.approval_id.as_ref(), draft["id"]);
        assert!(matches!(
            rig.plane
                .engine
                .reattach_by_correlation(proposed_write.approval_id.to_string())
                .unwrap(),
            ReceiptReattachment::NotFound
        ));
        assert!(matches!(
            rig.plane
                .engine
                .reattach_by_correlation("quick-gm-1".to_owned())
                .unwrap(),
            ReceiptReattachment::NotFound
        ));

        let (write, completion, work) = proposal.into_parts();
        let approval_id = write.approval_id.to_string();
        let accepted = rig
            .plane
            .accept_write(write, completion.into_receipt_sink())
            .expect("native approval must create the single durable receipt");
        drop(work);
        rig.registry.close_session(rig.context.id);
        rig.plane.activate_local_account(&second).unwrap();
        rig.plane.logout_local_account().unwrap();

        let ReceiptReattachment::Attached {
            id: receipt_id,
            statuses,
            ..
        } = rig
            .plane
            .engine
            .reattach_by_correlation(approval_id.clone())
            .unwrap()
        else {
            panic!("accepted Quick GM must have one canonical NMP receipt");
        };
        let expected_event = UnsignedEvent::new(
            PublicKey::from_str(&first.account.0).unwrap(),
            nmp::Timestamp::from(1_u64),
            nmp::Kind::TextNote,
            Vec::new(),
            "GM".to_owned(),
        );
        let expected_event_id = nmp::EventId::new(
            &expected_event.pubkey,
            &expected_event.created_at,
            &expected_event.kind,
            &expected_event.tags,
            &expected_event.content,
        );
        let mut signed_event_id = None;
        for _ in 0..32 {
            match statuses.recv_timeout(Duration::from_secs(2)).unwrap() {
                WriteStatus::Accepted | WriteStatus::AwaitingCapability { .. } => {}
                WriteStatus::Signed(event_id) => {
                    signed_event_id = Some(event_id);
                    break;
                }
                WriteStatus::Cancelled => {
                    panic!("session teardown must not cancel an accepted Quick GM")
                }
                WriteStatus::Failed(reason) => panic!("Quick GM failed before signing: {reason}"),
                _ => {}
            }
        }
        assert_eq!(
            signed_event_id.expect("Quick GM must be signed"),
            expected_event_id,
            "account changes after acceptance must not retarget the frozen event",
        );
        assert_eq!(accepted.receipt_id.0.as_ref(), receipt_id.0.to_string());

        let ReceiptReattachment::Attached {
            id: same_receipt_id,
            ..
        } = rig
            .plane
            .engine
            .reattach_by_correlation(approval_id)
            .unwrap()
        else {
            panic!("correlation must keep resolving to the canonical receipt");
        };
        assert_eq!(same_receipt_id, receipt_id);
        rig.plane.close();
    }
}
