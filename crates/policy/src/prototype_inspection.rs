//! PROTOTYPE(content-inspection): the `Inspection::Content` information provider.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use dogwood_language::{
    Arg, ParsedPolicySet, ProviderDeclarations, ProviderRequest, ProviderResolver, Value,
};

// PROTOTYPE(content-inspection): the provider keys the policy names.
pub(crate) const CONTENT_KEY: &str = "Inspection::Content";
pub(crate) const DOMAIN_KEY: &str = "Inspection::Domain";
pub(crate) const ADDRESS_KEY: &str = "Inspection::Address";
const PROVIDER_KEYS: [&str; 3] = [CONTENT_KEY, DOMAIN_KEY, ADDRESS_KEY];

// PROTOTYPE(content-inspection): interface-only declarations, so the resolver supplies the value.
const PROVIDERS_JSON: &str = r#"{
  "availableProviders": {
    "Inspection::Content": {
      "argumentTypes": [ { "paramType": "set", "items": { "paramType": "string" } } ],
      "outputType": {
        "paramType": "record",
        "fields": { "labels": { "paramType": "set", "items": { "paramType": "string" } } },
        "required": ["labels"]
      }
    },
    "Inspection::Domain": {
      "argumentTypes": [ { "paramType": "set", "items": { "paramType": "string" } } ],
      "outputType": {
        "paramType": "record",
        "fields": { "labels": { "paramType": "set", "items": { "paramType": "string" } } },
        "required": ["labels"]
      }
    },
    "Inspection::Address": {
      "argumentTypes": [ { "paramType": "set", "items": { "paramType": "string" } } ],
      "outputType": {
        "paramType": "record",
        "fields": { "labels": { "paramType": "set", "items": { "paramType": "string" } } },
        "required": ["labels"]
      }
    }
  }
}"#;

thread_local! {
    // PROTOTYPE(content-inspection): labels the inspector found, per provider, for the request being decided on this thread.
    static PENDING: RefCell<BTreeMap<&'static str, Vec<String>>> = const { RefCell::new(BTreeMap::new()) };
}

/// PROTOTYPE(content-inspection): the labels the loaded policy asks each provider about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectionLabels {
    /// `Inspection::Content` on `http:request`, sent as an `http` subject.
    pub content: Option<BTreeSet<String>>,
    /// `Inspection::Domain` on `net:resolve`, sent as a `dns` subject.
    pub domain: Option<BTreeSet<String>>,
    /// `Inspection::Address` on `net:connect`, sent as a `connect` subject.
    pub address: Option<BTreeSet<String>>,
}

// PROTOTYPE(content-inspection): the declarations every service schema carries.
pub(crate) fn declarations() -> ProviderDeclarations {
    ProviderDeclarations::from_json(PROVIDERS_JSON).expect("prototype provider declarations parse")
}

// PROTOTYPE(content-inspection): the resolver installed on every durable engine.
pub(crate) fn resolver() -> Arc<dyn ProviderResolver + Send + Sync> {
    Arc::new(InspectionResolver)
}

// PROTOTYPE(content-inspection): run `decide` with `labels` visible to the resolver for `key`.
pub(crate) fn with_labels<T>(
    key: &'static str,
    labels: &[String],
    decide: impl FnOnce() -> T,
) -> T {
    PENDING.with(|slot| {
        slot.borrow_mut().insert(key, labels.to_vec());
    });
    let result = decide();
    PENDING.with(|slot| slot.borrow_mut().clear());
    result
}

// PROTOTYPE(content-inspection): labels requested by each inspection provider in `source`.
pub(crate) fn requested_labels(
    source: &str,
    service: &dogwood_language::ServiceSchema,
) -> InspectionLabels {
    let mut found = InspectionLabels::default();
    let Ok(parsed) = ParsedPolicySet::parse(source, service) else {
        return found;
    };
    for policy in parsed.policies() {
        let Ok(invocations) = policy.provider_invocations_with_args() else {
            continue;
        };
        for invocation in invocations {
            let slot = match invocation.key().as_str() {
                CONTENT_KEY => &mut found.content,
                DOMAIN_KEY => &mut found.domain,
                ADDRESS_KEY => &mut found.address,
                _ => continue,
            };
            let labels = slot.get_or_insert_with(BTreeSet::new);
            for arg in &invocation.args {
                collect(arg, labels);
            }
        }
    }
    found
}

fn collect(arg: &Arg, labels: &mut BTreeSet<String>) {
    match arg {
        Arg::String(label) => {
            labels.insert(label.clone());
        }
        Arg::Set(items) => items.iter().for_each(|item| collect(item, labels)),
        _ => {}
    }
}

// PROTOTYPE(content-inspection): answers from the thread's pending labels, filtered to the requested set.
struct InspectionResolver;

impl ProviderResolver for InspectionResolver {
    fn resolve(&self, request: ProviderRequest<'_>) -> Option<Result<Value, String>> {
        let key = PROVIDER_KEYS
            .into_iter()
            .find(|key| *key == request.key())?;
        let requested: BTreeSet<String> = request
            .args
            .iter()
            .flat_map(|arg| match arg {
                Value::Array(items) => items.clone(),
                other => vec![other.clone()],
            })
            .filter_map(|value| match value {
                Value::String(label) => Some(label),
                _ => None,
            })
            .collect();
        let found = PENDING
            .with(|slot| slot.borrow().get(key).cloned())
            .unwrap_or_default();
        let labels = found
            .into_iter()
            .filter(|label| requested.contains(label))
            .map(Value::String)
            .collect();
        let mut record = std::collections::BTreeMap::new();
        record.insert("labels".to_string(), Value::Array(labels));
        Some(Ok(Value::Object(record)))
    }
}

/// PROTOTYPE(content-inspection): one outbound request handed to a content inspector.
#[derive(Debug)]
pub struct InspectionRequest<'a> {
    /// The labels the loaded policy asks about.
    pub labels: Vec<String>,
    /// The request method.
    pub method: &'a str,
    /// The request URL, without its query.
    pub url: String,
    /// The workload's headers.
    pub headers: &'a [(String, String)],
    /// The request body.
    pub body: &'a [u8],
}

/// PROTOTYPE(content-inspection): the kind of a name subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameSubjectKind {
    /// `dns`: a host name, before it is resolved.
    Dns,
    /// `connect`: one resolved address, before the socket opens.
    Connect,
}

/// PROTOTYPE(content-inspection): one `dns` or `connect` subject handed to an inspector.
#[derive(Debug)]
pub struct NameInspection<'a> {
    /// The subject kind.
    pub kind: NameSubjectKind,
    /// The labels the loaded policy asks about.
    pub labels: Vec<String>,
    /// The host name.
    pub name: &'a str,
    /// The resolved address, on `connect` only.
    pub ip: Option<std::net::IpAddr>,
    /// The destination port.
    pub port: u16,
}

/// PROTOTYPE(content-inspection): what a content inspector found.
#[derive(Debug, Clone, Default)]
pub struct InspectionVerdict {
    /// Whether the answer came from the `cache_ttl_s` cache.
    pub cached: bool,
    /// The labels found in the request.
    pub labels: Vec<String>,
    /// The inspector that classified the request, as its capabilities name it.
    pub backend: String,
    /// The end-to-end latency of the classify call.
    pub latency_ms: u64,
}

/// PROTOTYPE(content-inspection): classifies an outbound request; an error denies it.
pub trait ContentInspector: Send + Sync {
    /// Classify one request.
    fn inspect(&self, request: &InspectionRequest<'_>) -> Result<InspectionVerdict, String>;
    /// The largest body the inspector accepts; a larger one is denied, never truncated.
    fn max_body_bytes(&self) -> usize;
    /// Classify one `dns` or `connect` subject.
    fn inspect_name(&self, request: &NameInspection<'_>) -> Result<InspectionVerdict, String> {
        let _ = request;
        Err("this inspector does not classify names".to_string())
    }
}
