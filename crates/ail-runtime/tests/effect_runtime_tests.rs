use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ail_change::parser::parse_changeset;
use ail_compiler::core_ir::{LiteralValue, StageHashes};
use ail_compiler::{AnfBinding, AnfExpr, AnfIr, SourceMap, emit_wasm};
use ail_core::semantic_graph::NodeRef;
use ail_runtime::{
    AuditEvent, CapabilityGrant, CapabilityId, CapabilityManifest, ClockHandler, Handler,
    HostResult, LogHandler, ResourceLimits, RuntimeHost, RuntimeProfile, RuntimeValue,
    blake3_hex_of,
};

struct CountingHandler {
    calls: AtomicUsize,
    caps: Vec<CapabilityId>,
}

impl CountingHandler {
    fn new(cap: CapabilityId) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            caps: vec![cap],
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Handler for CountingHandler {
    fn name(&self) -> &str {
        "counting-handler"
    }

    fn capabilities(&self) -> &[CapabilityId] {
        &self.caps
    }

    fn handle(
        &self,
        _capability: &CapabilityId,
        _operation: &str,
        payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut arg = [0u8; 8];
        arg.copy_from_slice(&payload[..8]);
        let result = i64::from_le_bytes(arg) + 1;
        Ok(result.to_le_bytes().to_vec())
    }
}

fn effect_anf(capability: &str) -> AnfIr {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "n".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(41))),
            body: Box::new(AnfExpr::EffectCall {
                capability: capability.to_string(),
                func: "inc".to_string(),
                args: vec!["n".to_string()],
            }),
        },
    };
    AnfIr {
        schema_version: ail_compiler::anf::ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(std::slice::from_ref(&binding)),
        bindings: vec![binding],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: Some([2u8; 32]),
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    }
}

fn profile_for(wasm: &[u8], manifest: &CapabilityManifest, cap: CapabilityId) -> RuntimeProfile {
    RuntimeProfile::new(
        "effect-runtime-test".to_string(),
        blake3_hex_of(wasm),
        "a".repeat(64),
        manifest.blake3_hex().expect("manifest hash"),
        vec![CapabilityGrant {
            module: manifest.module.clone(),
            capability: cap,
        }],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    )
    .with_handler_binding_required()
}

#[test]
fn compiled_effect_dispatches_to_registered_handler_and_audits_call() {
    let acl = r#"
change effect-runtime-test
author test
base 0
ops
  op create_capability id=test.counter
end
end
"#;
    let parsed = parse_changeset(acl).expect("create_capability ACL parses");
    assert_eq!(parsed.parsed_ops[0].verb, "create_capability");
    let cap = CapabilityId::new(parsed.parsed_ops[0].args["id"].clone());

    let artifact = emit_wasm(&effect_anf(cap.as_str())).expect("effect wasm emits");
    let manifest = CapabilityManifest {
        module: "effect-module".to_string(),
        requires: vec![cap.clone()],
    };
    let profile = profile_for(&artifact.wasm, &manifest, cap.clone());
    let handler = Arc::new(CountingHandler::new(cap));

    let mut host = RuntimeHost::new().with_handler(handler.clone());
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("preflight passes");

    let result = instance.invoke("main", &[]).expect("main invokes");
    assert_eq!(result, RuntimeValue::I64(42));
    assert_eq!(handler.calls(), 1);

    let audit = instance.audit_log();
    let events = audit.events();
    assert!(events.iter().any(|event| matches!(
        event,
        AuditEvent::CapabilityCallExecuted {
            operation,
            handler_name,
            succeeded: true,
            ..
        } if operation == "inc" && handler_name == "counting-handler"
    )));
}

#[test]
fn builtin_log_and_clock_handlers_are_registered_handlers() {
    let log = LogHandler::new();
    let clock = ClockHandler::new();

    assert_eq!(log.capabilities()[0].as_str(), "log");
    assert_eq!(clock.capabilities()[0].as_str(), "clock");
    let now = clock
        .handle(&CapabilityId::new("clock"), "now", &[])
        .expect("clock handler succeeds");
    assert_eq!(now.len(), 8);
}
