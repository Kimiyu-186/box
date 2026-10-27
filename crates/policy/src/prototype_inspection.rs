//! PROTOTYPE(content-inspection): the `Inspection::Content` information provider.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use dogwood_language::{
    Arg, ParsedPolicySet, ProviderDeclarations, ProviderRequest, ProviderResolver, Value,
};

// PROTOTYPE(content-inspection): the provider key the policy names.
pub(crate) const PROVIDER_KEY: &str = "Inspection::Content";

// PROTOTYPE(content-inspection): interface-only declaration, so the resolver supplies the value.
const PROVIDERS_JSON: &str = r#"{
  "availableProviders": {
    "Inspection::Content": {
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
    // PROTOTYPE(content-inspection): labels the inspector found for the request being decided on this thread.
    static PENDING: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

// PROTOTYPE(content-inspection): the declarations every service schema carries.
pub(crate) fn declarations() -> ProviderDeclarations {
    ProviderDeclarations::from_json(PROVIDERS_JSON).expect("prototype provider declarations parse")
}

// PROTOTYPE(content-inspection): the resolver installed on every durable engine.
pub(crate) fn resolver() -> Arc<dyn ProviderResolver + Send + Sync> {
    Arc::new(InspectionResolver)
}

// PROTOTYPE(content-inspection): run `decide` with `labels` visible to the resolver.
pub(crate) fn with_labels<T>(labels: &[String], decide: impl FnOnce() -> T) -> T {
    PENDING.with(|slot| *slot.borrow_mut() = Some(labels.to_vec()));
    let result = decide();
    PENDING.with(|slot| *slot.borrow_mut() = None);
    result
}

// PROTOTYPE(content-inspection): labels requested by any `Inspection::Content(...)` call in `source`.
pub(crate) fn requested_labels(
    source: &str,
    service: &dogwood_language::ServiceSchema,
) -> Option<BTreeSet<String>> {
    let parsed = ParsedPolicySet::parse(source, service).ok()?;
    let mut labels = BTreeSet::new();
    let mut used = false;
    for policy in parsed.policies() {
        let Ok(invocations) = policy.provider_invocations_with_args() else {
            continue;
        };
        for invocation in invocations {
            if invocation.key() != PROVIDER_KEY {
                continue;
            }
            used = true;
            for arg in &invocation.args {
                collect(arg, &mut labels);
            }
        }
    }
    used.then_some(labels)
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
        if request.key() != PROVIDER_KEY {
            return None;
        }
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
        let found = PENDING.with(|slot| slot.borrow().clone()).unwrap_or_default();
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

/// PROTOTYPE(content-inspection): what a content inspector found.
#[derive(Debug, Clone, Default)]
pub struct InspectionVerdict {
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
}
