use super::helpers::*;
use ail_runtime::{
    HostDispatchResultDiagnostic, HostDispatchResultDiagnosticKind,
    sort_host_dispatch_result_diagnostics,
};

struct ShortResultHandler {
    calls: AtomicUsize,
    caps: Vec<CapabilityId>,
}

impl ShortResultHandler {
    fn new(cap: CapabilityId) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            caps: vec![cap],
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Handler for ShortResultHandler {
    fn name(&self) -> &str {
        "short-result-handler"
    }

    fn capabilities(&self) -> &[CapabilityId] {
        &self.caps
    }

    fn handle(
        &self,
        _capability: &CapabilityId,
        _operation: &str,
        _payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(vec![1, 2, 3, 4])
    }
}

fn direct_host_call_wasm(cap_name: &str, args_ptr: i32, args_len: i32) -> Vec<u8> {
    const CAP_PTR: i32 = 0;
    const OP_PTR: i32 = 64;

    let mut module = Module::new();

    let mut types = TypeSection::new();
    types.ty().function(
        vec![
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
        ],
        vec![ValType::I64],
    );
    types.ty().function(vec![], vec![ValType::I64]);
    module.section(&types);

    let mut imports = ImportSection::new();
    imports.import("ail", "host_call", EntityType::Function(0));
    module.section(&imports);

    let mut functions = FunctionSection::new();
    functions.function(1);
    module.section(&functions);

    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&memories);

    let mut exports = ExportSection::new();
    exports.export("memory", ExportKind::Memory, 0);
    exports.export("main", ExportKind::Func, 1);
    module.section(&exports);

    let mut codes = CodeSection::new();
    let mut function = Function::new(vec![]);
    function.instruction(&Instruction::I32Const(CAP_PTR));
    function.instruction(&Instruction::I32Const(cap_name.len() as i32));
    function.instruction(&Instruction::I32Const(OP_PTR));
    function.instruction(&Instruction::I32Const(2));
    function.instruction(&Instruction::I32Const(args_ptr));
    function.instruction(&Instruction::I32Const(args_len));
    function.instruction(&Instruction::Call(0));
    function.instruction(&Instruction::End);
    codes.function(&function);
    module.section(&codes);

    let mut data = DataSection::new();
    data.active(0, &ConstExpr::i32_const(CAP_PTR), cap_name.bytes());
    data.active(0, &ConstExpr::i32_const(OP_PTR), "op".bytes());
    module.section(&data);

    module.finish()
}

fn instantiate_direct_host_call(
    cap: &CapabilityId,
    args_ptr: i32,
    args_len: i32,
    handlers: Vec<Arc<dyn Handler + Send + Sync>>,
) -> (RuntimeHost, ail_runtime::RuntimeInstance) {
    let wasm = direct_host_call_wasm(cap.as_str(), args_ptr, args_len);
    let manifest = CapabilityManifest {
        module: "dispatch-result-diagnostics".to_string(),
        requires: vec![cap.clone()],
    };
    let profile = profile_granting(&wasm, &manifest, cap.clone());
    let mut host = RuntimeHost::new();
    for handler in handlers {
        host = host.with_handler(handler);
    }
    let instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight passes");
    (host, instance)
}

#[test]
fn successful_host_call_preserves_result_and_has_no_result_diagnostics() {
    let cap = CapabilityId::new("diagnostics.success");
    let handler = Arc::new(TrackingHandler::new("tracking-handler", cap.clone()));
    let (_host, mut instance) = instantiate_direct_host_call(&cap, 128, 0, vec![handler.clone()]);

    let result = instance.invoke("main", &[]).expect("invoke succeeds");

    assert_eq!(result, RuntimeValue::I64(99));
    assert_eq!(handler.call_count(), 1);
    assert!(instance.host_dispatch_result_diagnostics().is_empty());
}

#[test]
fn malformed_args_are_recorded_as_redacted_diagnostics() {
    let cap = CapabilityId::new("diagnostics.malformed-args");
    let handler = Arc::new(TrackingHandler::new("tracking-handler", cap.clone()));
    let (_host, mut instance) = instantiate_direct_host_call(&cap, 70_000, 1, vec![handler]);

    let _ = instance.invoke("main", &[]);

    let diagnostics = instance.host_dispatch_result_diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        HostDispatchResultDiagnosticKind::MalformedArgs
    );
    assert_eq!(diagnostics[0].classification, "args.memory");
    assert!(!diagnostics[0].subject.contains(cap.as_str()));
}

#[test]
fn missing_handler_is_recorded_as_redacted_diagnostic() {
    let cap = CapabilityId::new("diagnostics.missing-handler");
    let (_host, mut instance) = instantiate_direct_host_call(&cap, 128, 0, vec![]);

    let result = instance.invoke("main", &[]).expect("invoke succeeds");

    assert_eq!(result, RuntimeValue::I64(-1));
    let diagnostics = instance.host_dispatch_result_diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        HostDispatchResultDiagnosticKind::HandlerMissing
    );
    assert_eq!(diagnostics[0].classification, "handler.missing");
    assert!(!diagnostics[0].diagnostic_key.contains(cap.as_str()));
}

#[test]
fn handler_errors_are_recorded_without_leaking_messages() {
    let cap = CapabilityId::new("diagnostics.handler-error");
    let handler = Arc::new(FailingHandler::new("failing-handler", cap.clone()));
    let (_host, mut instance) = instantiate_direct_host_call(&cap, 128, 0, vec![handler.clone()]);

    let result = instance.invoke("main", &[]).expect("invoke succeeds");

    assert_eq!(result, RuntimeValue::I64(-1));
    assert_eq!(handler.call_count(), 1);
    let diagnostics = instance.host_dispatch_result_diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        HostDispatchResultDiagnosticKind::HostError
    );
    assert_eq!(diagnostics[0].classification, "host_error.custom");
    assert!(!diagnostics[0].detail.contains("handler failed"));
}

#[test]
fn short_host_call_response_is_diagnosed_without_changing_return_value() {
    let cap = CapabilityId::new("diagnostics.short-result");
    let handler = Arc::new(ShortResultHandler::new(cap.clone()));
    let (_host, mut instance) = instantiate_direct_host_call(&cap, 128, 0, vec![handler.clone()]);

    let result = instance.invoke("main", &[]).expect("invoke succeeds");

    assert_eq!(result, RuntimeValue::I64(0));
    assert_eq!(handler.call_count(), 1);
    let diagnostics = instance.host_dispatch_result_diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        HostDispatchResultDiagnosticKind::ResultAbiMismatch
    );
    assert_eq!(diagnostics[0].classification, "result.i64_bytes");
    assert_eq!(diagnostics[0].detail, "expected_bytes=8; actual_bytes=4");
}

#[test]
fn diagnostics_batch_sorting_is_deterministic_and_deduplicated() {
    let high = HostDispatchResultDiagnostic {
        kind: HostDispatchResultDiagnosticKind::HostError,
        diagnostic_key: "z/key".to_string(),
        subject: "z".to_string(),
        classification: "z".to_string(),
        detail: "z".to_string(),
    };
    let low = HostDispatchResultDiagnostic {
        kind: HostDispatchResultDiagnosticKind::MalformedArgs,
        diagnostic_key: "a/key".to_string(),
        subject: "a".to_string(),
        classification: "a".to_string(),
        detail: "a".to_string(),
    };

    let diagnostics = sort_host_dispatch_result_diagnostics(vec![high.clone(), low.clone(), high]);

    assert_eq!(diagnostics, vec![low, high]);
}
