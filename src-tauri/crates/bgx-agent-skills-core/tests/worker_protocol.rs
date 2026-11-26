//! W00: SKILL worker protocol matches `agent-skill-worker.json`; positive/negative message JSON;
//! the old WIT `produce-plan` input/output maps onto `producePlan` / `planProduced`.

use bgx_agent_skills_core::worker_protocol::*;
use serde_json::{json, Value};

const CONTRACT: &str =
    include_str!("../../../../packages/app-contracts/v1/agent-skill-worker.json");
const LIMITS: &str =
    include_str!("../../../../packages/app-contracts/v1/content-skill-limits.json");
const GOLDEN_PLAN: &[u8] = include_bytes!(
    "../../../../tests/fixtures/agent-skills/reference/rust-abi-reference/references/workflow.json"
);

fn contract() -> Value {
    serde_json::from_str(CONTRACT).unwrap()
}

#[test]
fn generated_constants_follow_both_contracts() {
    let c = contract();
    let l: Value = serde_json::from_str(LIMITS).unwrap();
    assert_eq!(
        PROTOCOL_VERSION as u64,
        c["protocolVersion"].as_u64().unwrap()
    );
    assert_eq!(
        FRAME_MAX_BYTES as u64,
        c["framing"]["frameMaxBytes"].as_u64().unwrap()
    );
    assert_eq!(
        COMPONENT_MAX_BYTES as u64,
        c["componentMaxBytes"].as_u64().unwrap()
    );
    assert_eq!(MAX_INFLIGHT_REQUESTS, 1);
    assert_eq!(
        WASM_MEMORY_BYTES as u64,
        l["wasm"]["memoryBytes"].as_u64().unwrap()
    );
    assert_eq!(WASM_FUEL, l["wasm"]["fuel"].as_u64().unwrap());
    assert_eq!(WASM_WALL_MS, l["wasm"]["wallMs"].as_u64().unwrap());
    assert_eq!(
        INPUT_MAX_BYTES as u64,
        l["maxSnapshotBytes"].as_u64().unwrap()
    );
    assert_eq!(OUTPUT_MAX_BYTES as u64, l["maxPlanBytes"].as_u64().unwrap());
    let allowed: Vec<&str> = l["allowedHostImports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(ALLOWED_IMPORTS, allowed.as_slice());
    assert_eq!(ABI_WORLD, c["abi"]["world"].as_str().unwrap());
    assert_eq!(c["abi"]["status"], "helper-compat");
}

#[test]
fn every_contract_code_is_generated_and_partitioned() {
    let c = contract();
    let w = c["workerErrorCodes"].as_object().unwrap();
    let t = c["transportErrorCodes"].as_object().unwrap();
    assert_eq!(w.len() + t.len(), WorkerErrorCode::ALL.len());
    for (k, m) in w {
        let code = WorkerErrorCode::parse(k).unwrap();
        assert!(!code.is_transport(), "{k}");
        assert_eq!(code.meaning(), m.as_str().unwrap());
    }
    for k in t.keys() {
        assert!(WorkerErrorCode::parse(k).unwrap().is_transport(), "{k}");
    }
    assert!(WorkerErrorCode::parse("AGENT_SKILL_WORKER_NOPE").is_none());
}

fn sample_parent() -> Vec<ParentMessage> {
    let blob = Blob::from_bytes(b"\0asm");
    vec![
        ParentMessage::Hello {
            protocol_version: 1,
        },
        ParentMessage::ValidateComponent {
            request_id: "r1".into(),
            component: blob.clone(),
        },
        ParentMessage::ProducePlan {
            request_id: "r2".into(),
            component: blob.clone(),
            input: Blob::from_bytes(b"{}"),
        },
        ParentMessage::Cancel {
            request_id: "r2".into(),
        },
        ParentMessage::Shutdown {},
    ]
}

fn sample_worker() -> Vec<WorkerMessage> {
    vec![
        WorkerMessage::HelloAck {
            protocol_version: 1,
            contract_sha256: contract_sha256(),
            worker_version: "0.1.0".into(),
            limits: WorkerLimits::CONTRACT,
            abi_world: ABI_WORLD.into(),
        },
        WorkerMessage::Validated {
            request_id: "r1".into(),
            component_sha256: sha256_ref(b"\0asm"),
            imports: vec![ALLOWED_IMPORTS[0].into()],
        },
        WorkerMessage::PlanProduced {
            request_id: "r2".into(),
            component_sha256: sha256_ref(b"\0asm"),
            input_sha256: sha256_ref(b"{}"),
            output: Blob::from_bytes(GOLDEN_PLAN),
            fuel_consumed: 10,
        },
        WorkerMessage::Failed {
            request_id: Some("r3".into()),
            code: WorkerErrorCode::Trap.as_str().into(),
            message: "unreachable".into(),
        },
        WorkerMessage::CancelAck {
            request_id: "r2".into(),
            active: true,
        },
        WorkerMessage::ShutdownAck {},
    ]
}

/// Serialized messages carry exactly the contract `type` names and required fields.
fn check_against_contract(msgs: Vec<Value>, table: &[(&str, &[&str])], key: &str) {
    let c = contract();
    assert_eq!(table.len(), c[key].as_object().unwrap().len());
    let mut seen = std::collections::BTreeSet::new();
    for v in msgs {
        let ty = v["type"].as_str().unwrap().to_string();
        let (_, fields) = table
            .iter()
            .find(|(t, _)| *t == ty)
            .unwrap_or_else(|| panic!("{ty} not in contract {key}"));
        let obj = v.as_object().unwrap();
        for f in *fields {
            assert!(obj.contains_key(*f), "{ty} missing {f}");
        }
        assert_eq!(obj.len(), fields.len() + 1, "{ty} has extra fields: {v}");
        seen.insert(ty);
    }
    assert_eq!(seen.len(), table.len(), "every {key} type sampled");
}

#[test]
fn message_shapes_match_contract_tables() {
    check_against_contract(
        sample_parent()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .collect(),
        PARENT_MESSAGES,
        "parentToWorker",
    );
    check_against_contract(
        sample_worker()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .collect(),
        WORKER_MESSAGES,
        "workerToParent",
    );
}

#[test]
fn frames_roundtrip_every_message() {
    for m in sample_parent() {
        let f = encode_frame(&m).unwrap();
        let len = frame_body_len(f[..4].try_into().unwrap()).unwrap();
        assert_eq!(len, f.len() - 4);
        assert_eq!(decode_frame_body::<ParentMessage>(&f[4..]).unwrap(), m);
    }
    for m in sample_worker() {
        let f = encode_frame(&m).unwrap();
        assert_eq!(decode_frame_body::<WorkerMessage>(&f[4..]).unwrap(), m);
    }
}

#[test]
fn negative_message_json_is_rejected() {
    let blob = serde_json::to_value(Blob::from_bytes(b"x")).unwrap();
    let cases = [
        json!({"type":"hello"}),
        json!({"type":"hello","protocolVersion":"1"}),
        json!({"type":"hello","protocolVersion":1,"extra":true}),
        json!({"type":"producePlan","requestId":"r","component":blob}),
        json!({"type":"producePlan","requestId":"r","component":blob,"input":{"bytesBase64":"eA=="}}),
        json!({"type":"producePlan","requestId":"r","component":blob,"input":blob,"limits":{"fuel":1}}),
        json!({"type":"execScript","requestId":"r"}),
        json!({"requestId":"r"}),
        json!({"type":"validate_component","requestId":"r","component":blob}),
        json!([1, 2]),
    ];
    for c in cases {
        let bytes = serde_json::to_vec(&c).unwrap();
        let err = decode_frame_body::<ParentMessage>(&bytes).unwrap_err();
        assert_eq!(err.code(), WorkerErrorCode::FrameMalformed, "{c}");
    }
    assert!(decode_frame_body::<ParentMessage>(b"not json").is_err());
}

#[test]
fn old_wit_produce_plan_fixture_maps_onto_protocol() {
    // WIT: produce-plan(snapshot-json: list<u8>) -> result<list<u8>, string>
    let snapshot = br#"{"storyId":"s1"}"#;
    let req = ParentMessage::ProducePlan {
        request_id: "r".into(),
        component: Blob::from_bytes(include_bytes!(
            "../../../../tests/fixtures/agent-skills/reference/rust-abi-reference/scripts/helper.wasm"
        )),
        input: Blob::from_bytes(snapshot),
    };
    let ParentMessage::ProducePlan {
        input, component, ..
    } = &req
    else {
        unreachable!()
    };
    assert_eq!(
        input
            .decode_verified(INPUT_MAX_BYTES, WorkerErrorCode::InputTooLarge)
            .unwrap(),
        snapshot
    );
    assert!(component
        .decode_verified(COMPONENT_MAX_BYTES, WorkerErrorCode::ComponentTooLarge)
        .unwrap()
        .starts_with(b"\0asm"));
    // result::ok(list<u8>) -> planProduced.output (raw bytes, host accepts the plan).
    let ok = WorkerMessage::PlanProduced {
        request_id: "r".into(),
        component_sha256: component.sha256.clone(),
        input_sha256: input.sha256.clone(),
        output: Blob::from_bytes(GOLDEN_PLAN),
        fuel_consumed: 1,
    };
    let WorkerMessage::PlanProduced { output, .. } = &ok else {
        unreachable!()
    };
    let plan: Value = serde_json::from_slice(
        &output
            .decode_verified(OUTPUT_MAX_BYTES, WorkerErrorCode::OutputTooLarge)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(plan["protocol"], "content-skill-v1");
    // result::err(string) -> failed GUEST_ERROR with the guest text.
    let err = WorkerMessage::failed(
        Some("r"),
        &WorkerFailure::new(WorkerErrorCode::GuestError, "bad snapshot"),
    );
    assert_eq!(err.terminal_request_id(), Some("r"));
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(v["code"], "AGENT_SKILL_WORKER_GUEST_ERROR");
    assert_eq!(v["message"], "bad snapshot");
}

#[test]
fn contract_digest_has_hash_format() {
    let d = contract_sha256();
    assert!(d.starts_with("sha256:"));
    assert_eq!(d.len(), 7 + 64);
    assert_eq!(d, sha256_ref(CONTRACT.replace("\r\n", "\n").as_bytes()));
}
