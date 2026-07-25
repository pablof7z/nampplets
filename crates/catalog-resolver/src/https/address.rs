//! Security-critical public-address validation.
//!
//! Every predicate in this file is moved byte-for-byte from the original
//! single-file implementation: no reordering, no rewording, no
//! "simplification" of the RFC1918/loopback/link-local/CGNAT range checks.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

use super::AcquisitionRefusal;

pub(crate) fn validate_candidate(
    candidate: &str,
    maximum_url_bytes: usize,
) -> Result<Url, AcquisitionRefusal> {
    if candidate.len() > maximum_url_bytes {
        return Err(AcquisitionRefusal::InvalidCandidate);
    }
    let url = Url::parse(candidate).map_err(|_| AcquisitionRefusal::InvalidCandidate)?;
    if url.scheme() != "https"
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AcquisitionRefusal::NonHttps);
    }
    match url.host() {
        Some(Host::Ipv4(address)) if !is_public_ip(IpAddr::V4(address)) => {
            Err(AcquisitionRefusal::NonPublicAddress {
                address: IpAddr::V4(address),
            })
        }
        Some(Host::Ipv6(address)) if !is_public_ip(IpAddr::V6(address)) => {
            Err(AcquisitionRefusal::NonPublicAddress {
                address: IpAddr::V6(address),
            })
        }
        Some(Host::Domain(domain))
            if domain.eq_ignore_ascii_case("localhost")
                || domain.to_ascii_lowercase().ends_with(".localhost") =>
        {
            Err(AcquisitionRefusal::NonHttps)
        }
        Some(_) => Ok(url),
        None => Err(AcquisitionRefusal::InvalidCandidate),
    }
}

pub(crate) fn validate_resolved_addresses(
    addresses: &[IpAddr],
    maximum: usize,
) -> Result<(), AcquisitionRefusal> {
    if addresses.is_empty() {
        return Err(AcquisitionRefusal::MissingAddressEvidence);
    }
    if addresses.len() > maximum {
        return Err(AcquisitionRefusal::AddressLimit {
            actual: addresses.len(),
            maximum,
        });
    }
    for address in addresses.iter().copied() {
        if !is_public_ip(address) {
            return Err(AcquisitionRefusal::NonPublicAddress { address });
        }
    }
    Ok(())
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || address == Ipv4Addr::BROADCAST)
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    if address.is_unspecified() || address.is_loopback() || address.is_multicast() {
        return false;
    }
    let segments = address.segments();
    if segments[0] & 0xfe00 == 0xfc00
        || segments[0] & 0xffc0 == 0xfe80
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
    {
        return false;
    }
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_public_ipv4(mapped);
    }
    true
}
