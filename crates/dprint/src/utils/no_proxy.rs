// Code in this module was lifted from https://github.com/denoland/deno/blob/e6869d7fa668017bacf23ad80a52a4168f562e7b/ext/fetch/proxy.rs#L235
// Copyright 2018-2024 the Deno authors. MIT license.
use std::net::IpAddr;

use ipnet::IpNet;

/// Represents a possible matching entry for an IP address
#[derive(Clone, Debug)]
enum Ip {
  Address(IpAddr),
  Network(IpNet),
}

/// A wrapper around a list of IP cidr blocks or addresses with a [IpMatcher::contains] method for
/// checking if an IP address is contained within the matcher
#[derive(Clone, Debug, Default)]
struct IpMatcher(Vec<Ip>);

/// A wrapper around a list of domains with a [DomainMatcher::contains] method for checking if a
/// domain is contained within the matcher
#[derive(Clone, Debug, Default)]
struct DomainMatcher(Vec<String>);

#[derive(Debug)]
enum NoProxyInner {
  Some { domains: DomainMatcher, ips: IpMatcher },
  None,
}

#[derive(Debug)]
pub struct NoProxy(NoProxyInner);

impl NoProxy {
  /// Returns a new no-proxy configuration based on environment variables
  /// see [self::NoProxy::from_string()] for the string format
  pub fn from_env() -> NoProxy {
    // ok to use because we only use this when constructing the environment
    #[allow(clippy::disallowed_methods)]
    let raw = std::env::var("NO_PROXY").or_else(|_| std::env::var("no_proxy")).unwrap_or_default();

    Self::from_string(&raw)
  }

  /// Returns a new no-proxy configuration based on a `no_proxy` string.
  /// The rules are as follows:
  /// * The environment variable `NO_PROXY` is checked, if it is not set, `no_proxy` is checked
  /// * If neither environment variable is set, `None` is returned
  /// * Entries are expected to be comma-separated (whitespace between entries is ignored)
  /// * IP addresses (both IPv4 and IPv6) are allowed, as are optional subnet masks (by adding /size,
  ///   for example "`192.168.1.0/24`").
  /// * An entry "`*`" matches all hostnames (this is the only wildcard allowed)
  /// * Any other entry is considered a domain name (and may contain a leading dot, for example `google.com`
  ///   and `.google.com` are equivalent) and would match both that domain AND all subdomains.
  ///
  /// For example, if `"NO_PROXY=google.com, 192.168.1.0/24"` was set, all of the following would match
  /// (and therefore would bypass the proxy):
  /// * `http://google.com/`
  /// * `http://www.google.com/`
  /// * `http://192.168.1.42/`
  ///
  /// The URL `http://notgoogle.com/` would not match.
  pub fn from_string(no_proxy_list: &str) -> Self {
    let no_proxy_list = no_proxy_list.trim();
    if no_proxy_list.is_empty() {
      return Self(NoProxyInner::None);
    }
    let mut ips = Vec::new();
    let mut domains = Vec::new();
    let parts = no_proxy_list.split(',').map(str::trim);
    for part in parts {
      match part.parse::<IpNet>() {
        // If we can parse an IP net or address, then use it, otherwise, assume it is a domain
        Ok(ip) => ips.push(Ip::Network(ip)),
        Err(_) => match part.parse::<IpAddr>() {
          Ok(addr) => ips.push(Ip::Address(addr)),
          Err(_) => domains.push(part.to_owned()),
        },
      }
    }
    Self(NoProxyInner::Some {
      ips: IpMatcher(ips),
      domains: DomainMatcher(domains),
    })
  }

  pub fn contains(&self, host: &str) -> bool {
    match &self.0 {
      NoProxyInner::Some { domains, ips } => {
        // According to RFC3986, raw IPv6 hosts will be wrapped in []. So we need to strip those off
        // the end in order to parse correctly
        let host = if host.starts_with('[') {
          let x: &[_] = &['[', ']'];
          host.trim_matches(x)
        } else {
          host
        };
        match host.parse::<IpAddr>() {
          // If we can parse an IP addr, then use it, otherwise, assume it is a domain
          Ok(ip) => ips.contains(ip),
          Err(_) => domains.contains(host),
        }
      }
      NoProxyInner::None => false,
    }
  }
}

impl IpMatcher {
  fn contains(&self, addr: IpAddr) -> bool {
    for ip in &self.0 {
      match ip {
        Ip::Address(address) => {
          if &addr == address {
            return true;
          }
        }
        Ip::Network(net) => {
          if net.contains(&addr) {
            return true;
          }
        }
      }
    }
    false
  }
}

impl DomainMatcher {
  // The following links may be useful to understand the origin of these rules:
  // * https://curl.se/libcurl/c/CURLOPT_NOPROXY.html
  // * https://github.com/curl/curl/issues/1208
  fn contains(&self, domain: &str) -> bool {
    let domain_len = domain.len();
    for d in &self.0 {
      if d == domain || d.strip_prefix('.') == Some(domain) {
        return true;
      } else if domain.ends_with(d) {
        if d.starts_with('.') {
          // If the first character of d is a dot, that means the first character of domain
          // must also be a dot, so we are looking at a subdomain of d and that matches
          return true;
        } else if domain.as_bytes().get(domain_len - d.len() - 1) == Some(&b'.') {
          // Given that d is a prefix of domain, if the prior character in domain is a dot
          // then that means we must be matching a subdomain of d, and that matches
          return true;
        }
      } else if d == "*" {
        return true;
      }
    }
    false
  }
}

#[cfg(test)]
mod test {
  use super::DomainMatcher;
  use super::NoProxy;

  #[test]
  fn test_domain_matcher() {
    let domains = vec![".foo.bar".into(), "bar.foo".into()];
    let matcher = DomainMatcher(domains);

    // domains match with leading `.`
    assert!(matcher.contains("foo.bar"));
    // subdomains match with leading `.`
    assert!(matcher.contains("www.foo.bar"));

    // domains match with no leading `.`
    assert!(matcher.contains("bar.foo"));
    // subdomains match with no leading `.`
    assert!(matcher.contains("www.bar.foo"));

    // non-subdomain string prefixes don't match
    assert!(!matcher.contains("notfoo.bar"));
    assert!(!matcher.contains("notbar.foo"));
  }

  #[test]
  fn test_no_proxy_wildcard() {
    let no_proxy = NoProxy::from_string("*");
    assert!(no_proxy.contains("any.where"));
  }

  #[test]
  fn test_no_proxy_ip_ranges() {
    let no_proxy = NoProxy::from_string(".foo.bar, bar.baz,10.42.1.1/24,::1,10.124.7.8,2001::/17");

    let should_not_match = [
      // random url, not in no_proxy
      "deno.com",
      // make sure that random non-subdomain string prefixes don't match
      "notfoo.bar",
      // make sure that random non-subdomain string prefixes don't match
      "notbar.baz",
      // ipv4 address out of range
      "10.43.1.1",
      // ipv4 address out of range
      "10.124.7.7",
      // ipv6 address out of range
      "[ffff:db8:a0b:12f0::1]",
      // ipv6 address out of range
      "[2005:db8:a0b:12f0::1]",
    ];

    for host in &should_not_match {
      assert!(!no_proxy.contains(host), "should not contain {:?}", host);
    }

    let should_match = [
      // make sure subdomains (with leading .) match
      "hello.foo.bar",
      // make sure exact matches (without leading .) match (also makes sure spaces between entries work)
      "bar.baz",
      // make sure subdomains (without leading . in no_proxy) match
      "foo.bar.baz",
      // make sure subdomains (without leading . in no_proxy) match - this differs from cURL
      "foo.bar",
      // ipv4 address match within range
      "10.42.1.100",
      // ipv6 address exact match
      "[::1]",
      // ipv6 address match within range
      "[2001:db8:a0b:12f0::1]",
      // ipv4 address exact match
      "10.124.7.8",
    ];

    for host in &should_match {
      assert!(no_proxy.contains(host), "should contain {:?}", host);
    }
  }
}
