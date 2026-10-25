//! PROTOTYPE(content-inspection): smoke check that `Inspection::Content` loads and decides.

use policy::{GovernedBox, Policy, PolicyEngine, Principal, Request};

const SOURCE: &str = r#"
permit (principal, action == Box::Action::"http:request", resource);
forbid (principal, action == Box::Action::"http:request", resource)
when { Inspection::Content(["PII", "SECRET"]).labels.containsAny(["PII", "SECRET"]) };
"#;

#[test]
fn inspection_labels_drive_the_forbid() {
    let dir = tempfile::tempdir().unwrap();
    let engine = PolicyEngine::open(
        vec![Policy { origin: "p.dw".into(), text: SOURCE.to_string() }],
        &dir.path().join("h.redb"),
    )
    .expect("opens");
    assert_eq!(
        engine.uses_inspection().unwrap().into_iter().collect::<Vec<_>>(),
        vec!["PII".to_string(), "SECRET".to_string()]
    );
    let request = Request::Http {
        host: "h", port: 443, method: "POST", path: "/", body_bytes: 1, intercepted: true,
    };
    let g = GovernedBox::assigned("b");
    let p = Principal::agent();
    let clean = engine.decide_with_inspection(&g, &p, &request, &[]);
    let dirty = engine.decide_with_inspection(&g, &p, &request, &["PII".to_string()]);
    let other = engine.decide_with_inspection(&g, &p, &request, &["OTHER".to_string()]);
    let plain = engine.decide(&g, &p, &request);
    eprintln!("clean={clean} dirty={dirty} other={other} plain={plain}");
    assert!(clean.is_allow());
    assert!(!dirty.is_allow());
    assert!(other.is_allow());
    assert!(plain.is_allow());
}
