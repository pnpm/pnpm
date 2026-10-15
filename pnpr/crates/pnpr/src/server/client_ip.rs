use axum::http::HeaderMap;
use pnpr_config::{IpNetwork, TrustedProxyHeader};
use std::net::{IpAddr, SocketAddr};

/// Client IP address resolved for the current request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClientIp(pub(crate) IpAddr);

/// Determine the client IP address from the direct socket peer and request
/// forwarding headers.
///
/// If `peer` is `None`, or if `peer` is not within `trusted_proxies`, forwarding
/// headers are ignored and `peer` is returned (failing closed if `None`).
///
/// If `peer` is within `trusted_proxies`, exactly one configured forwarding header
/// (`trusted_proxy_header`) is inspected. If the header is missing or cannot be
/// parsed, `None` is returned so security-sensitive checks fail closed rather
/// than attributing the request to the trusted proxy.
pub(crate) fn resolve_client_ip(
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    trusted_proxies: &[IpNetwork],
    header_type: TrustedProxyHeader,
) -> Option<IpAddr> {
    let peer = peer?.to_canonical();
    if trusted_proxies.is_empty() || !is_trusted(peer, trusted_proxies) {
        return Some(peer);
    }

    match header_type {
        TrustedProxyHeader::Forwarded => forwarded_client_ip(headers, trusted_proxies),
        TrustedProxyHeader::XForwardedFor => x_forwarded_for_client_ip(headers, trusted_proxies),
        TrustedProxyHeader::XRealIp => x_real_ip(headers),
    }
}

fn is_trusted(ip: IpAddr, trusted_proxies: &[IpNetwork]) -> bool {
    trusted_proxies
        .iter()
        .any(|net| net.contains(ip))
}

fn forwarded_client_ip(headers: &HeaderMap, trusted_proxies: &[IpNetwork]) -> Option<IpAddr> {
    let mut hops = Vec::new();
    for value in headers.get_all("forwarded") {
        let text = value.to_str().ok()?;
        let elements = split_quoted(text, ',')?;
        for element in elements {
            let raw = element.trim();
            if !raw.is_empty() {
                hops.push(parse_forwarded_element(raw));
            }
        }
    }
    if hops.is_empty() {
        return None;
    }
    right_to_left_walk(&hops, trusted_proxies)
}

fn parse_forwarded_element(element: &str) -> Option<IpAddr> {
    let params = split_quoted(element, ';')?;
    for param in params {
        let (key, val) = param.trim().split_once('=')?;
        if key.trim().eq_ignore_ascii_case("for") {
            return parse_hop_ip(val);
        }
    }
    None
}

fn split_quoted(text: &str, delimiter: char) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;

    for (index, character) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if quoted && character == '\\' {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character == delimiter && !quoted {
            parts.push(&text[start..index]);
            start = index + character.len_utf8();
        }
    }

    if quoted || escaped {
        return None;
    }

    parts.push(&text[start..]);
    Some(parts)
}

fn x_forwarded_for_client_ip(headers: &HeaderMap, trusted_proxies: &[IpNetwork]) -> Option<IpAddr> {
    let mut hops = Vec::new();
    for value in headers.get_all("x-forwarded-for") {
        let text = value.to_str().ok()?;
        for element in text.split(',') {
            let raw = element.trim();
            if !raw.is_empty() {
                hops.push(parse_hop_ip(raw));
            }
        }
    }
    if hops.is_empty() {
        return None;
    }
    right_to_left_walk(&hops, trusted_proxies)
}

fn x_real_ip(headers: &HeaderMap) -> Option<IpAddr> {
    let raw = headers.get("x-real-ip")?.to_str().ok()?;
    parse_hop_ip(raw.trim())
}

/// Traverse proxy hops from right to left (closest proxy to origin).
///
/// Hops matching `trusted_proxies` are skipped. The first untrusted hop
/// encountered is returned as the client IP. If a hop is malformed (`None`),
/// traversal stops and fails closed (returns `None` so the caller falls back to peer).
/// If all hops are trusted, the leftmost hop is returned.
fn right_to_left_walk(hops: &[Option<IpAddr>], trusted_proxies: &[IpNetwork]) -> Option<IpAddr> {
    for hop in hops.iter().rev() {
        let Some(ip) = hop else {
            return None;
        };
        if !is_trusted(*ip, trusted_proxies) {
            return Some(*ip);
        }
    }
    hops.first().copied().flatten()
}

fn parse_hop_ip(raw: &str) -> Option<IpAddr> {
    let raw = raw.trim();
    let raw = if let Some(unquoted) = raw.strip_prefix('"') {
        let stripped = unquoted.strip_suffix('"')?;
        if stripped.contains('"') {
            return None;
        }
        stripped
    } else if raw.contains('"') {
        return None;
    } else {
        raw
    };
    if let Ok(ip) = raw.parse::<IpAddr>() {
        return Some(ip.to_canonical());
    }
    if let Ok(socket_addr) = raw.parse::<SocketAddr>() {
        return Some(socket_addr.ip().to_canonical());
    }
    if let Some(inner) = raw
        .strip_prefix('[')
        .and_then(|bracketed| bracketed.strip_suffix(']'))
    {
        return inner
            .parse::<IpAddr>()
            .ok()
            .map(|ip| ip.to_canonical());
    }
    None
}

#[cfg(test)]
mod tests;
