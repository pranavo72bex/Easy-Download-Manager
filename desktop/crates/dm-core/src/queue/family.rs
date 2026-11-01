/// Whether two addresses are of the same family of servers: the same host, or
/// hosts that share their registered name (`cdn1.example.com` and
/// `dl.example.com`). A signed address often moves between a site's own
/// servers when it is issued again; it does not move to another site.
pub(super) fn same_family(a: &str, b: &str) -> bool {
    matches!((family(a), family(b)), (Some(x), Some(y)) if x == y)
}

fn family(url: &str) -> Option<String> {
    let host = reqwest::Url::parse(url)
        .ok()?
        .host_str()?
        .to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    // An address with no names in it (an IP) is its own family.
    if labels.len() < 3 || host.parse::<std::net::IpAddr>().is_ok() {
        return Some(host);
    }
    // `example.co.uk`: a short second-level label under a two-letter country
    // code belongs to the registry, not to the site.
    let n = labels.len();
    let registry = labels[n - 1].len() == 2
        && matches!(
            labels[n - 2],
            "co" | "com" | "org" | "net" | "gov" | "edu" | "ac"
        );
    let keep = if registry { 3 } else { 2 };
    Some(labels[n - keep..].join("."))
}

#[cfg(test)]
mod tests {
    use super::same_family;

    #[test]
    fn servers_of_one_site_are_a_family_and_other_sites_are_not() {
        assert!(same_family(
            "https://cdn1.example.com/a?x=1",
            "https://dl.example.com/b"
        ));
        assert!(same_family("http://127.0.0.1:80/a", "http://127.0.0.1:9/b"));
        assert!(same_family(
            "https://a.shop.example.co.uk/x",
            "https://b.shop.example.co.uk/y"
        ));
        assert!(!same_family(
            "https://example.com/a",
            "https://example.org/a"
        ));
        assert!(!same_family(
            "https://one.example.co.uk/a",
            "https://two.other.co.uk/a"
        ));
        assert!(!same_family("http://10.0.0.1/a", "http://10.0.0.2/a"));
        assert!(!same_family("not a url", "https://example.com/"));
    }
}
