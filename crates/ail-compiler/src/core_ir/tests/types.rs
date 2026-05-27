use super::helpers::*;

// TRIANGULATE: all CoreNodeKind variants are constructible.
// Ensures no variant is accidentally omitted from the enum.
#[test]
fn all_core_node_kinds_are_constructible() {
    let kinds = [
        CoreNodeKind::Module,
        CoreNodeKind::Function,
        CoreNodeKind::Type,
        CoreNodeKind::Effect,
        CoreNodeKind::Capability,
        CoreNodeKind::Contract,
        CoreNodeKind::Invariant,
        CoreNodeKind::Test,
        CoreNodeKind::Boundary,
        CoreNodeKind::Package,
    ];
    assert_eq!(
        kinds.len(),
        10,
        "all 10 CoreNodeKind variants must be reachable"
    );
}

// ── G2: CoreType tests ────────────────────────────────────────────────

// S2: All original CoreType variants are constructible without panic.
// Updated for ola3-core-ir-types: parameterized variants now carry inner types.
#[test]
fn all_core_type_variants_are_constructible() {
    // Original unit-like variants (unchanged).
    let _unit = CoreType::Unit;
    let _never = CoreType::Never;
    let _bool = CoreType::Bool;
    let _int = CoreType::Int;
    let _uint = CoreType::UInt;
    let _float = CoreType::Float;
    let _text = CoreType::Text;
    let _bytes = CoreType::Bytes;
    let _record = CoreType::Record;
    let _variant = CoreType::Variant;
    let _tuple = CoreType::Tuple;
    let _generic = CoreType::Generic(None);
    // Parameterized variants (now carry inner types).
    let _list = CoreType::List(Box::new(CoreType::Int));
    let _map = CoreType::Map(Box::new(CoreType::Text), Box::new(CoreType::Int));
    let _set = CoreType::Set(Box::new(CoreType::Bool));
    let _option = CoreType::Option(Box::new(CoreType::Int));
    let _result = CoreType::Result(Box::new(CoreType::Int), Box::new(CoreType::Text));
    let _function = CoreType::Function {
        params: vec![CoreType::Int],
        ret: Box::new(CoreType::Bool),
        effects: vec![],
    };
    let _handle = CoreType::Handle {
        resource: Box::new(CoreType::Text),
        mode: ResourceMode::Copy,
    };
    let _refinement = CoreType::Refinement {
        base: Box::new(CoreType::Int),
        predicate: "x > 0".to_string(),
    };
    // All constructed without panic — test passes.
}

// S1: CoreType::Bool is constructible and serializable (deterministic CBOR).
#[test]
fn core_type_bool_cbor_is_deterministic() {
    let ty = CoreType::Bool;
    let b1 = stable_cbor_bytes(&ty).expect("first encode");
    let b2 = stable_cbor_bytes(&ty).expect("second encode");
    assert_eq!(b1, b2, "CoreType::Bool CBOR must be deterministic");
}

// TRIANGULATE: different CoreType variants produce different CBOR bytes.
#[test]
fn different_core_types_produce_different_cbor() {
    let b_int = stable_cbor_bytes(&CoreType::Int).expect("encode Int");
    let b_text = stable_cbor_bytes(&CoreType::Text).expect("encode Text");
    assert_ne!(b_int, b_text, "Int and Text must produce different CBOR");
}

// ── G2: CoreExpr tests ────────────────────────────────────────────────

// S3: All CoreExpr variants are constructible without panic.

#[test]
fn list_with_inner_type_cbor_round_trip() {
    let ty = CoreType::List(Box::new(CoreType::Int));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "List<Int> must survive CBOR round-trip");
    // Verify inner type is preserved.
    if let CoreType::List(inner) = decoded {
        assert_eq!(*inner, CoreType::Int);
    } else {
        panic!("expected List variant");
    }
}

// S-B1b: Map(Text, Int) round-trips — both key and value types preserved.
#[test]
fn map_with_key_and_value_types_cbor_round_trip() {
    let ty = CoreType::Map(Box::new(CoreType::Text), Box::new(CoreType::Int));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "Map<Text, Int> must survive CBOR round-trip");
}

// S-B1c: Option(Bool) round-trips.
#[test]
fn option_with_inner_type_cbor_round_trip() {
    let ty = CoreType::Option(Box::new(CoreType::Bool));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "Option<Bool> must survive CBOR round-trip");
}

// S-B1d: Result(Int, Text) round-trips — Ok and Err types preserved.
#[test]
fn result_with_ok_and_err_types_cbor_round_trip() {
    let ty = CoreType::Result(Box::new(CoreType::Int), Box::new(CoreType::Text));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, ty,
        "Result<Int, Text> must survive CBOR round-trip"
    );
}

// S-B1e: Handle { resource: Text, mode: ResourceMode::Linear } round-trips.
#[test]
fn handle_with_resource_and_mode_cbor_round_trip() {
    let ty = CoreType::Handle {
        resource: Box::new(CoreType::Text),
        mode: ResourceMode::Linear,
    };
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, ty,
        "Handle<Text, Linear> must survive CBOR round-trip"
    );
    if let CoreType::Handle { resource, mode } = decoded {
        assert_eq!(*resource, CoreType::Text);
        assert_eq!(mode, ResourceMode::Linear);
    } else {
        panic!("expected Handle variant");
    }
}

// S-B1f: PatchField(Text) round-trips — new parameterized variant.
#[test]
fn patch_field_with_inner_type_cbor_round_trip() {
    let ty = CoreType::PatchField(Box::new(CoreType::Text));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "PatchField<Text> must survive CBOR round-trip");
}

// S-B1g: Vector(Float) round-trips.
#[test]
fn vector_with_inner_type_cbor_round_trip() {
    let ty = CoreType::Vector(Box::new(CoreType::Float));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "Vector<Float> must survive CBOR round-trip");
}

// S-B1h: OrderedSet(Int) round-trips.
#[test]
fn ordered_set_with_inner_type_cbor_round_trip() {
    let ty = CoreType::OrderedSet(Box::new(CoreType::Int));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "OrderedSet<Int> must survive CBOR round-trip");
}

// S-B1i: Task(Bool) round-trips — concurrency type with inner.
#[test]
fn task_with_inner_type_cbor_round_trip() {
    let ty = CoreType::Task(Box::new(CoreType::Bool));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "Task<Bool> must survive CBOR round-trip");
}

// S-B1j: Channel(Text) round-trips.
// Triangulation: Task<Bool> and Channel<Text> must produce different CBOR.
#[test]
fn channel_with_inner_type_cbor_round_trip() {
    let ty = CoreType::Channel(Box::new(CoreType::Text));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "Channel<Text> must survive CBOR round-trip");
    // Triangulation: Channel<Text> ≠ Task<Bool>
    let task_ty = CoreType::Task(Box::new(CoreType::Bool));
    let task_bytes = stable_cbor_bytes(&task_ty).expect("encode task");
    assert_ne!(
        bytes, task_bytes,
        "Channel<Text> must differ from Task<Bool> in CBOR"
    );
}

// ── Task A1 (RED): new flat CoreType variants ─────────────────────────

// S-A1a: All new flat CoreType variants are constructible.
#[test]
fn new_flat_core_type_variants_are_constructible() {
    let _decimal = CoreType::Decimal;
    let _existential = CoreType::Existential;
    let _code_point = CoreType::CodePoint;
    let _grapheme = CoreType::Grapheme;
    let _normalized_text = CoreType::NormalizedText("NFC".to_string());
    let _int32 = CoreType::Int32;
    let _int64 = CoreType::Int64;
    let _uint32 = CoreType::UInt32;
    let _uint64 = CoreType::UInt64;
    let _task_group = CoreType::TaskGroup;
    // All constructed without panic — test passes.
}

// S-A1b: NormalizedText carries its form string and round-trips through CBOR.
#[test]
fn normalized_text_cbor_round_trip() {
    let ty = CoreType::NormalizedText("NFC".to_string());
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, ty,
        "NormalizedText<NFC> must survive CBOR round-trip"
    );
}

// S-A1c: Decimal is distinct from Int and Float in CBOR encoding.
// Triangulation: different flat numeric types must produce different CBOR.
#[test]
fn decimal_is_distinct_from_int_and_float_in_cbor() {
    let b_decimal = stable_cbor_bytes(&CoreType::Decimal).expect("encode Decimal");
    let b_int = stable_cbor_bytes(&CoreType::Int).expect("encode Int");
    let b_float = stable_cbor_bytes(&CoreType::Float).expect("encode Float");
    assert_ne!(b_decimal, b_int, "Decimal must differ from Int in CBOR");
    assert_ne!(b_decimal, b_float, "Decimal must differ from Float in CBOR");
}

// S-A1d: Machine integer variants are all distinct from each other.
// Triangulation: Int32/Int64/UInt32/UInt64 must encode differently.
#[test]
fn machine_integer_variants_are_distinct_in_cbor() {
    let b_i32 = stable_cbor_bytes(&CoreType::Int32).expect("encode Int32");
    let b_i64 = stable_cbor_bytes(&CoreType::Int64).expect("encode Int64");
    let b_u32 = stable_cbor_bytes(&CoreType::UInt32).expect("encode UInt32");
    let b_u64 = stable_cbor_bytes(&CoreType::UInt64).expect("encode UInt64");
    assert_ne!(b_i32, b_i64);
    assert_ne!(b_i32, b_u32);
    assert_ne!(b_i32, b_u64);
    assert_ne!(b_i64, b_u32);
    assert_ne!(b_i64, b_u64);
    assert_ne!(b_u32, b_u64);
}

// ── Task A3 (RED): new additive CoreExpr variants ─────────────────────

// S-A3a: All new CoreExpr variants are constructible.

#[test]
fn dyn_core_type_construction_and_eq() {
    let ty = CoreType::Dyn("Serializable".to_string());
    // Eq: same interface name → equal
    assert_eq!(ty, CoreType::Dyn("Serializable".to_string()));
    // Eq: different interface → not equal
    assert_ne!(ty, CoreType::Dyn("Repository<User>".to_string()));
}

// A1-2: CoreType::Dyn CBOR round-trip preserves the interface name.
// Spec scenario: "Dyn CoreType construction and CBOR round-trip"
#[test]
fn dyn_core_type_cbor_round_trip() {
    let ty = CoreType::Dyn("Serializable".to_string());
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, ty,
        "Dyn<Serializable> must survive CBOR round-trip"
    );
    if let CoreType::Dyn(name) = decoded {
        assert_eq!(name, "Serializable");
    } else {
        panic!("expected Dyn variant after round-trip");
    }
}

// A1-3 (TRIANGULATE): Dyn with a generic interface name also round-trips.
// Forces the payload to be a non-trivial string, not a hardcoded empty string.
#[test]
fn dyn_core_type_with_generic_interface_cbor_round_trip() {
    let ty = CoreType::Dyn("Repository<User>".to_string());
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty);
    if let CoreType::Dyn(name) = decoded {
        assert_eq!(name, "Repository<User>");
    } else {
        panic!("expected Dyn variant");
    }
}

// A1-4: CoreExpr::DynCall construction and field access.
// Spec scenario: "DynCall construction and CBOR round-trip"

#[test]
fn boundary_schema_cbor_round_trip() {
    let ty = CoreType::BoundarySchema("UserInputJsonSchema".to_string());
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "BoundarySchema must survive CBOR round-trip");
    if let CoreType::BoundarySchema(name) = decoded {
        assert_eq!(name, "UserInputJsonSchema");
    } else {
        panic!("expected BoundarySchema variant");
    }
}

// F1-2: BoundarySchema Eq — same name equals, different names do not.
#[test]
fn boundary_schema_variant_equality() {
    let a = CoreType::BoundarySchema("UserInputJsonSchema".to_string());
    let b = CoreType::BoundarySchema("UserInputJsonSchema".to_string());
    let c = CoreType::BoundarySchema("PaymentsJsonSchema".to_string());
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// F1-3 (TRIANGULATE): BoundarySchema is distinct from Dyn and ForeignType in CBOR.
#[test]
fn boundary_schema_is_distinct_from_dyn_and_foreign_type_in_cbor() {
    let bs = stable_cbor_bytes(&CoreType::BoundarySchema("Schema".to_string()))
        .expect("encode BoundarySchema");
    let dyn_ = stable_cbor_bytes(&CoreType::Dyn("Schema".to_string())).expect("encode Dyn");
    let foreign = stable_cbor_bytes(&CoreType::ForeignType("Schema".to_string()))
        .expect("encode ForeignType");
    // Same payload string but different variants → different CBOR
    assert_ne!(bs, dyn_, "BoundarySchema must differ from Dyn in CBOR");
    assert_ne!(
        bs, foreign,
        "BoundarySchema must differ from ForeignType in CBOR"
    );
}

// S-A3e: ForEach, Fold, Return, MapNew, SetNew, IndexGet round-trip through CBOR.
