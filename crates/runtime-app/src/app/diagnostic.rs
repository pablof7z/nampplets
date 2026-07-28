//! Runtime-owned classification of napplet diagnostic envelopes.
//!
//! A sandboxed napplet mirrors its own console output to the host. That
//! message carries no NAP domain authority — but *deciding* that it carries
//! none is a protocol-membership judgement, and this crate owns those. A
//! shell that compares the caller-supplied `type` against a literal of its
//! own is asserting a fact it does not own, on input the napplet controls.
//!
//! So the kernel asserts it instead. `debug.*` is reserved: it is never a
//! negotiated capability, never reaches a provider, and always produces a
//! typed, bounded fact rather than vanishing.

/// The reserved domain for host-visible diagnostics. Reserved means the
/// runtime answers for it: `Capability::new("debug")` never becomes a
/// negotiated domain, so nothing here can be granted, dispatched, or
/// injected.
pub(crate) const DIAGNOSTIC_DOMAIN: &str = "debug";
pub(crate) const CONSOLE_ACTION: &str = "console";

/// The most bytes of a napplet-supplied diagnostic message the runtime will
/// carry. The trusted shell already truncates at its own bound; a napplet can
/// post past that shell entirely, so the kernel bounds it again.
const MAXIMUM_DIAGNOSTIC_MESSAGE_BYTES: usize = 4 * 1024;

/// Diagnostics one session may mirror to the host.
///
/// Mirrors the trusted shell's own console cap. That cap lives inside the
/// sandboxed frame, so a napplet posting the envelope directly never meets it;
/// this is the copy that holds regardless. It never resets within a session,
/// which is exactly the shell's behaviour, so a legitimate napplet cannot
/// reach it any sooner than it already would.
pub(crate) const MAXIMUM_SESSION_DIAGNOSTICS: u32 = 500;

/// The severity of one diagnostic, as a closed set the runtime owns.
///
/// The napplet names a level; it does not get to widen the set. Anything
/// outside it is [`NappletDiagnosticLevel::Unknown`] — which is a verdict,
/// not a passthrough of the string the napplet chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NappletDiagnosticLevel {
    Log,
    Info,
    Warn,
    Error,
    Debug,
    /// The napplet named a level this runtime does not recognise.
    Unknown,
}

impl NappletDiagnosticLevel {
    fn parse(value: &str) -> Self {
        match value {
            "log" => Self::Log,
            "info" => Self::Info,
            "warn" => Self::Warn,
            "error" => Self::Error,
            "debug" => Self::Debug,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Log => "log",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Debug => "debug",
            Self::Unknown => "unknown",
        }
    }
}

/// What one `debug.*` envelope turned out to be.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DiagnosticEnvelope {
    /// A readable console entry.
    Console {
        level: NappletDiagnosticLevel,
        message: String,
    },
    /// The envelope claimed the reserved domain but could not be read as a
    /// diagnostic. It is still classified — and still recorded — rather than
    /// dropped, because a napplet whose diagnostics silently vanish is
    /// exactly the situation diagnostics exist to prevent.
    Unreadable { reason: &'static str },
}

/// Classifies an envelope in the reserved diagnostic domain.
///
/// `None` means the envelope is not diagnostic and belongs to the ordinary
/// protocol path. The decision is made here, from the parsed envelope, so no
/// caller has to compare a type string of its own.
pub(crate) fn classify_diagnostic(bytes: &[u8]) -> Option<DiagnosticEnvelope> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let message_type = value.get("type")?.as_str()?;
    let (domain, action) = message_type.split_once('.')?;
    if domain != DIAGNOSTIC_DOMAIN {
        return None;
    }
    if action != CONSOLE_ACTION {
        return Some(DiagnosticEnvelope::Unreadable {
            reason: "unknown-diagnostic-action",
        });
    }
    let Some(message) = value.get("message").and_then(serde_json::Value::as_str) else {
        return Some(DiagnosticEnvelope::Unreadable {
            reason: "missing-or-non-string-message",
        });
    };
    // A missing level is not a failure to read the entry: the message is the
    // part that matters, and an unnamed severity is honestly unknown.
    let level = value
        .get("level")
        .and_then(serde_json::Value::as_str)
        .map_or(NappletDiagnosticLevel::Unknown, |level| {
            NappletDiagnosticLevel::parse(level)
        });
    Some(DiagnosticEnvelope::Console {
        level,
        message: super::envelope::bounded_utf8_prefix(message, MAXIMUM_DIAGNOSTIC_MESSAGE_BYTES),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_envelope_is_not_diagnostic() {
        assert_eq!(
            classify_diagnostic(br#"{"type":"identity.getPublicKey"}"#),
            None
        );
        assert_eq!(classify_diagnostic(br#"{"type":"shell.ready"}"#), None);
        assert_eq!(classify_diagnostic(b"{ not json"), None);
    }

    /// `debug` is reserved wholesale, so an unknown action under it is still
    /// the runtime's to answer for.
    #[test]
    fn an_unknown_action_in_the_reserved_domain_is_still_classified() {
        assert_eq!(
            classify_diagnostic(br#"{"type":"debug.trace"}"#),
            Some(DiagnosticEnvelope::Unreadable {
                reason: "unknown-diagnostic-action"
            })
        );
    }

    #[test]
    fn a_level_outside_the_closed_set_becomes_unknown() {
        assert_eq!(
            classify_diagnostic(br#"{"type":"debug.console","level":"catastrophe","message":"x"}"#),
            Some(DiagnosticEnvelope::Console {
                level: NappletDiagnosticLevel::Unknown,
                message: "x".to_owned()
            })
        );
    }

    #[test]
    fn a_message_longer_than_the_bound_is_truncated() {
        let huge = "a".repeat(MAXIMUM_DIAGNOSTIC_MESSAGE_BYTES * 2);
        let envelope = format!(r#"{{"type":"debug.console","level":"error","message":"{huge}"}}"#);
        let Some(DiagnosticEnvelope::Console { message, .. }) =
            classify_diagnostic(envelope.as_bytes())
        else {
            panic!("an oversized console entry is still a console entry");
        };
        assert_eq!(message.len(), MAXIMUM_DIAGNOSTIC_MESSAGE_BYTES);
    }
}
