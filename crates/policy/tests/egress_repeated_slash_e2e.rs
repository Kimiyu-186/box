//! A path rule judges a request path with each run of `/` merged, and the gateway forwards the path
//! as the workload sent it, through the real MITM gateway and the real policy engine.

#![cfg(feature = "egress-adapter")]

#[path = "../../egress-gateway/tests/harness/mod.rs"]
mod harness;

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use egress_gateway::{CapabilitySet, MitmConfig, MitmHandle, MitmInterceptor};
use policy::{EgressPolicyInterceptor, GovernedBox, Policy, Principal};

use harness::{TlsUpstream, WorkloadClient};

const HOST: &str = "api.slash.test";

/// A gateway for one TLS upstream, governed by `rules` plus a permit for every connect.
fn gateway(upstream: &TlsUpstream, rules: &str) -> MitmHandle {
    let policy = support::open_policy(vec![Policy {
        origin: PathBuf::from("repeated-slash.dw"),
        text: format!(
            r#"permit(principal, action == Box::Action::"net:connect", resource);
{rules}"#
        ),
    }])
    .expect("the policy opens");
    let interceptor = EgressPolicyInterceptor::into_handle(
        Arc::new(policy),
        Principal::agent(),
        GovernedBox::assigned("test-box"),
    );
    let config = MitmConfig {
        upstream_ca_pems: vec![upstream.ca_pem()],
        dns_overrides: vec![(HOST.to_string(), "127.0.0.1".parse().unwrap())],
        ..MitmConfig::default()
    };
    MitmInterceptor::start(config, CapabilitySet::default(), interceptor).expect("gateway starts")
}

/// The status the gateway answers for `GET target`.
fn get(handle: &MitmHandle, upstream: &TlsUpstream, target: &str) -> u16 {
    let port = handle.port().expect("a TCP gateway has a port");
    let mut client = WorkloadClient::connect(port, HOST, upstream.port());
    client.send_request("GET", target, &[], b"");
    client.read_status()
}

/// The request line the upstream read last.
fn forwarded_request_line(upstream: &TlsUpstream) -> String {
    let raw = upstream.last_request().raw;
    raw.split("\r\n").next().unwrap_or_default().to_string()
}

#[test]
fn a_path_forbid_covers_each_repeated_slash_spelling_of_the_path() {
    let upstream = TlsUpstream::start(HOST);
    let handle = gateway(
        &upstream,
        r#"permit(principal, action == Box::Action::"http:request", resource);
forbid(principal, action == Box::Action::"http:request", resource)
when { context.input.path like "/v1/admin*" };"#,
    );

    assert_eq!(get(&handle, &upstream, "/v1/ok"), 200, "control");
    assert_eq!(get(&handle, &upstream, "/v1/admin"), 403, "control");
    for spelling in [
        "/v1//admin",
        "/v1///admin/export",
        "/v1//./admin",
        "/v1/admin//",
    ] {
        assert_eq!(get(&handle, &upstream, spelling), 403, "{spelling}");
    }
    assert_eq!(
        upstream.request_count(),
        1,
        "no forbidden spelling reached the origin"
    );
}

#[test]
fn a_repeated_slash_path_is_forwarded_byte_identical() {
    let upstream = TlsUpstream::start(HOST);
    let handle = gateway(
        &upstream,
        r#"permit(principal, action == Box::Action::"http:request", resource);"#,
    );

    assert_eq!(get(&handle, &upstream, "/bucket//key//?a=%2F%2F"), 200);
    assert_eq!(
        forwarded_request_line(&upstream),
        "GET /bucket//key//?a=%2F%2F HTTP/1.1"
    );
}

#[test]
fn a_repeated_slash_request_to_a_host_with_no_path_rule_goes_through_unchanged() {
    let upstream = TlsUpstream::start(HOST);
    let handle = gateway(
        &upstream,
        &format!(
            r#"permit(principal, action == Box::Action::"http:request", resource)
when {{ context.input.host == "{HOST}" }};"#
        ),
    );

    assert_eq!(get(&handle, &upstream, "/a//b//"), 200);
    assert_eq!(forwarded_request_line(&upstream), "GET /a//b// HTTP/1.1");
}

#[test]
fn a_permit_also_admits_each_repeated_slash_neighbour_of_a_path_it_admits() {
    let upstream = TlsUpstream::start(HOST);
    let handle = gateway(
        &upstream,
        r#"permit(principal, action == Box::Action::"http:request", resource)
when { context.input.path == "/a/b" || context.input.path like "/p/q/*" };"#,
    );

    assert_eq!(get(&handle, &upstream, "/a/c"), 403, "control");
    assert_eq!(get(&handle, &upstream, "/a//b"), 200);
    assert_eq!(forwarded_request_line(&upstream), "GET /a//b HTTP/1.1");
    assert_eq!(get(&handle, &upstream, "/p//q/r"), 200);
    assert_eq!(forwarded_request_line(&upstream), "GET /p//q/r HTTP/1.1");
}
