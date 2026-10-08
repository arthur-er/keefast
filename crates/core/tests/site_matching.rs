//! URL matching: which Entry URLs are for a web page. Registrable domains come from the Public
//! Suffix List; schemes, ports and IP addresses are matched strictly.

use keefast_core::Site;

fn site(url: &str) -> Site {
    Site::parse(url).unwrap_or_else(|| panic!("{url} is a web page"))
}

#[track_caller]
fn assert_matches(page: &str, entry_urls: &[&str]) {
    let site = site(page);
    for url in entry_urls {
        assert!(site.matches(url), "{url:?} should be for {page}");
    }
}

#[track_caller]
fn assert_no_match(page: &str, entry_urls: &[&str]) {
    let site = site(page);
    for url in entry_urls {
        assert!(!site.matches(url), "{url:?} should not be for {page}");
    }
}

#[test]
fn an_entry_is_for_its_host_and_subdomains() {
    assert_matches(
        "https://login.example.com/session?next=/",
        &[
            "https://example.com",
            "https://example.com/login",
            "example.com",
            "login.example.com",
            "HTTPS://EXAMPLE.COM",
            "https://example.com./",
        ],
    );
}

#[test]
fn an_entry_is_not_for_other_domains_or_its_parent() {
    assert_no_match(
        "https://example.com/",
        &[
            "https://example.org",
            "https://notexample.com",
            "https://login.example.com",
            "https://example.com.evil.net",
            "https://com",
        ],
    );
    assert_no_match("https://example.com.evil.net/", &["https://example.com"]);
}

#[test]
fn matching_never_crosses_a_public_suffix() {
    // co.uk is a public suffix: each domain under it is a different site.
    assert_matches("https://www.example.co.uk/", &["example.co.uk"]);
    assert_no_match("https://example.co.uk/", &["co.uk", "other.co.uk"]);
    // So is github.io, from the List's private section: each user's pages are their own site.
    assert_matches(
        "https://docs.alice.github.io/",
        &["alice.github.io", "docs.alice.github.io"],
    );
    assert_no_match("https://alice.github.io/", &["github.io", "bob.github.io"]);
}

#[test]
fn an_https_entry_is_never_for_an_http_page() {
    assert_no_match(
        "http://example.com/",
        &["https://example.com", "example.com", "login.example.com"],
    );
    // An http Entry is for the same site over either scheme.
    assert_matches("http://example.com/", &["http://example.com"]);
    assert_matches("https://example.com/", &["http://example.com"]);
}

#[test]
fn an_entry_with_a_port_is_only_for_that_port() {
    assert_matches(
        "https://example.com:8443/app",
        &["https://example.com:8443"],
    );
    assert_no_match(
        "https://example.com/",
        &["https://example.com:8443", "http://example.com:8080"],
    );
    // A default port is no port at all, and an Entry without a port is for any.
    assert_matches("https://example.com/", &["https://example.com:443"]);
    assert_matches("https://example.com:8443/", &["https://example.com"]);
    assert_matches("http://localhost:3000/", &["http://localhost:3000"]);
}

#[test]
fn ip_addresses_match_only_exactly() {
    assert_matches(
        "https://192.168.1.1/admin",
        &["192.168.1.1", "https://192.168.1.1/"],
    );
    assert_no_match(
        "https://192.168.1.10/",
        &["192.168.1.1", "168.1.10", "https://1.10"],
    );
    assert_matches(
        "http://[::1]:8080/",
        &["http://[::1]:8080", "http://[0:0:0:0:0:0:0:1]"],
    );
    assert_no_match("http://[::1]/", &["http://[::2]", "http://127.0.0.1"]);
}

#[test]
fn hosts_without_a_registrable_domain_match_only_exactly() {
    assert_matches("http://localhost/", &["http://localhost"]);
    assert_no_match("http://app.localhost/", &["http://localhost"]);
}

#[test]
fn only_http_and_https_pages_and_entries_take_part() {
    for page in [
        "file:///home/me/login.html",
        "chrome://extensions",
        "ftp://example.com/",
        "about:blank",
        "",
        "not a url",
    ] {
        assert_eq!(Site::parse(page), None, "{page:?}");
    }
    assert_no_match(
        "https://example.com/",
        &[
            "ftp://example.com",
            "androidapp://com.example",
            "",
            "   ",
            "https://",
        ],
    );
}

#[test]
fn a_site_is_known_by_its_host() {
    assert_eq!(
        site("https://Login.Example.com:8443/x").host(),
        "login.example.com"
    );
    assert_eq!(site("http://[::1]:8080/").host(), "::1");
}

#[test]
fn an_entry_url_names_the_site_its_favicon_comes_from() {
    let origin = |url: &str| Site::of_entry_url(url).map(|site| site.origin());

    assert_eq!(
        origin("https://login.example.com/session?next=/").as_deref(),
        Some("https://login.example.com")
    );
    assert_eq!(
        origin("example.com/login").as_deref(),
        Some("https://example.com")
    );
    assert_eq!(
        origin("  HTTP://Example.COM./ ").as_deref(),
        Some("http://example.com")
    );
    assert_eq!(
        origin("https://example.com:8443/").as_deref(),
        Some("https://example.com:8443")
    );
    assert_eq!(
        origin("https://[::1]:8080").as_deref(),
        Some("https://[::1]:8080")
    );
    assert_eq!(origin(""), None);
    assert_eq!(origin("ftp://example.com"), None);
    assert_eq!(origin("cmd://putty.exe"), None);
}
