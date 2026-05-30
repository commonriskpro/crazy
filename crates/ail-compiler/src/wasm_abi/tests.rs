use super::export_name;

//
// The arm_payload_binding function is imported from pattern_string and
// fully tested there. No duplicate tests are kept here.

#[test]
fn export_name_preserves_module_namespace_without_legacy_prefix() {
    assert_eq!(export_name("fn.main"), "main");
    assert_eq!(export_name("fn.app.main"), "app_main");
    assert_eq!(export_name("test.math.addition"), "math_addition");
}
