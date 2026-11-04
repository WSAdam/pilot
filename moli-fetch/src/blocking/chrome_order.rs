//! pilot: request headers on the wire in the order Chrome sends them.
//!
//! Bot defenses compare a request's header order with the browser its
//! User-Agent names. Moli builds its header list in its own order and
//! leaves User-Agent and Accept-Encoding to libcurl, which writes both
//! ahead of every other header. Here both join the list, and a
//! navigation's headers are sorted into Chrome's order.

/// What pilot accepts. Chrome also offers zstd, which this libcurl cannot
/// decode.
pub(super) const ACCEPT_ENCODING: &str = "gzip, deflate, br";
/// Chrome's priority for a top-level navigation (RFC 9218). pilot's
/// libcurl drops a Priority header on HTTP/1.x, where Chrome sends none.
const NAVIGATION_PRIORITY: &str = "u=0, i";

/// Chrome's order for a navigation's headers.
const NAVIGATION_ORDER: &[&str] = &[
    "cache-control",
    "sec-ch-ua",
    "sec-ch-ua-mobile",
    "sec-ch-ua-platform",
    "origin",
    "content-type",
    "upgrade-insecure-requests",
    "user-agent",
    "accept",
    "sec-fetch-site",
    "sec-fetch-mode",
    "sec-fetch-user",
    "sec-fetch-dest",
    "sec-fetch-storage-access",
    "referer",
    "accept-encoding",
    "accept-language",
    "cookie",
    "if-none-match",
    "if-modified-since",
    "priority",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Navigation {
    None,
    Subframe,
    TopLevel,
}

/// The headers for libcurl to send, from Moli's list: User-Agent and
/// Accept-Encoding added when missing, and a navigation's headers in
/// Chrome's order. Headers Chrome's order does not name keep their
/// relative order, just before Referer.
pub(super) fn wire_headers(
    mut headers: Vec<(String, Vec<u8>)>,
    user_agent: &str,
    navigation: Navigation,
) -> Vec<(String, Vec<u8>)> {
    let present = |headers: &[(String, Vec<u8>)], name: &str| {
        headers
            .iter()
            .any(|(header, _)| header.eq_ignore_ascii_case(name))
    };
    // Where libcurl would have written them: first.
    if !present(&headers, "accept-encoding") {
        headers.insert(0, ("Accept-Encoding".to_owned(), ACCEPT_ENCODING.into()));
    }
    if !user_agent.is_empty() && !present(&headers, "user-agent") {
        headers.insert(0, ("User-Agent".to_owned(), user_agent.into()));
    }
    if navigation == Navigation::None {
        return headers;
    }
    if navigation == Navigation::TopLevel && !present(&headers, "priority") {
        headers.push(("Priority".to_owned(), NAVIGATION_PRIORITY.into()));
    }
    let unnamed = 2 * position("referer").unwrap_or(NAVIGATION_ORDER.len()) - 1;
    headers.sort_by_key(|(name, _)| position(name).map_or(unnamed, |index| 2 * index));
    headers
}

fn position(name: &str) -> Option<usize> {
    NAVIGATION_ORDER
        .iter()
        .position(|known| known.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "Mozilla/5.0 Chrome/146.0.0.0";

    fn headers(names: &[&str]) -> Vec<(String, Vec<u8>)> {
        names
            .iter()
            .map(|name| ((*name).to_owned(), format!("{name} value").into_bytes()))
            .collect()
    }

    fn names(headers: &[(String, Vec<u8>)]) -> Vec<&str> {
        headers.iter().map(|(name, _)| name.as_str()).collect()
    }

    fn value<'a>(headers: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
        headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_slice())
    }

    #[test]
    fn a_navigation_is_sorted_into_chromes_order() {
        // Moli's own order for a top-level navigation with a cookie.
        let moli = headers(&[
            "Cookie",
            "Accept",
            "Accept-Language",
            "Upgrade-Insecure-Requests",
            "Sec-Fetch-Mode",
            "Sec-Fetch-Dest",
            "Sec-Fetch-Site",
            "Sec-Fetch-User",
            "Sec-CH-UA",
            "Sec-CH-UA-Mobile",
            "Sec-CH-UA-Platform",
            "Referer",
        ]);
        let wire = wire_headers(moli, UA, Navigation::TopLevel);
        assert_eq!(
            names(&wire),
            [
                "Sec-CH-UA",
                "Sec-CH-UA-Mobile",
                "Sec-CH-UA-Platform",
                "Upgrade-Insecure-Requests",
                "User-Agent",
                "Accept",
                "Sec-Fetch-Site",
                "Sec-Fetch-Mode",
                "Sec-Fetch-User",
                "Sec-Fetch-Dest",
                "Referer",
                "Accept-Encoding",
                "Accept-Language",
                "Cookie",
                "Priority",
            ]
        );
        assert_eq!(value(&wire, "user-agent"), Some(UA.as_bytes()));
        assert_eq!(
            value(&wire, "accept-encoding"),
            Some(b"gzip, deflate, br".as_slice())
        );
        assert_eq!(value(&wire, "priority"), Some(b"u=0, i".as_slice()));
    }

    #[test]
    fn headers_chrome_does_not_name_keep_their_order_before_referer() {
        let moli = headers(&["X-Second", "Referer", "Accept", "X-First", "x-second"]);
        let wire = wire_headers(moli, UA, Navigation::Subframe);
        assert_eq!(
            names(&wire),
            [
                "User-Agent",
                "Accept",
                "X-Second",
                "X-First",
                "x-second",
                "Referer",
                "Accept-Encoding",
            ]
        );
    }

    #[test]
    fn only_a_top_level_navigation_gets_chromes_priority() {
        for navigation in [Navigation::None, Navigation::Subframe] {
            let wire = wire_headers(headers(&["Accept"]), UA, navigation);
            assert_eq!(value(&wire, "priority"), None, "{navigation:?}");
        }
        let explicit = vec![("priority".to_owned(), b"u=3".to_vec())];
        let wire = wire_headers(explicit, UA, Navigation::TopLevel);
        assert_eq!(value(&wire, "priority"), Some(b"u=3".as_slice()));
    }

    #[test]
    fn a_subresource_keeps_moli_order_after_curls_two_headers() {
        let moli = headers(&["Accept", "Sec-Fetch-Mode", "Referer", "Sec-CH-UA"]);
        let wire = wire_headers(moli, UA, Navigation::None);
        assert_eq!(
            names(&wire),
            [
                "User-Agent",
                "Accept-Encoding",
                "Accept",
                "Sec-Fetch-Mode",
                "Referer",
                "Sec-CH-UA",
            ]
        );
    }

    #[test]
    fn an_explicit_user_agent_or_encoding_is_kept() {
        let moli = vec![
            ("accept-encoding".to_owned(), b"identity".to_vec()),
            ("USER-AGENT".to_owned(), b"Custom/1".to_vec()),
        ];
        let wire = wire_headers(moli, UA, Navigation::None);
        assert_eq!(names(&wire), ["accept-encoding", "USER-AGENT"]);
        assert_eq!(value(&wire, "user-agent"), Some(b"Custom/1".as_slice()));
        assert_eq!(
            value(&wire, "accept-encoding"),
            Some(b"identity".as_slice())
        );
    }

    #[test]
    fn an_empty_user_agent_is_left_out_as_libcurl_did() {
        let wire = wire_headers(Vec::new(), "", Navigation::None);
        assert_eq!(names(&wire), ["Accept-Encoding"]);
    }
}
