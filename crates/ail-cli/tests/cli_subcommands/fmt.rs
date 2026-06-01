// Mechanical split from cli_subcommands.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;
use predicates::prelude::*;

/// Spec scenario: ail fmt formats ACL into canonical command text.
///   GIVEN an ACL file with out-of-order ops and non-canonical function id
///   WHEN `ail fmt --file <path> --json` runs
///   THEN JSON includes formatted ACL with phase ordering and materialized defaults
#[test]
fn fmt_file_json_outputs_canonical_acl() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let acl = dir.child("change.acl");
    acl.write_str(
        "change x\nbase 0\nauthor Ana\nop verify\nop create_function id=Fn.CartTotal return=I64\nend\n",
    )
    .expect("write acl");

    let output = ail()
        .args(["fmt", "--file"])
        .arg(acl.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    let formatted = v["data"]["formatted"]
        .as_str()
        .expect("formatted must be string");
    assert!(
        formatted.contains("op create_function id=fn.cart_total return=I64 visibility=private"),
        "fmt must normalize id and materialize default visibility; got:\n{formatted}"
    );
    assert!(
        formatted.find("op create_function").unwrap() < formatted.find("op verify").unwrap(),
        "fmt must phase-order create before verify; got:\n{formatted}"
    );
    assert_eq!(v["data"]["changed"], true);
}
/// Spec scenario: ail fmt --write rewrites the file so --check passes.
#[test]
fn fmt_write_makes_check_pass() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let acl = dir.child("change.acl");
    acl.write_str("change x\nbase 0\nauthor Ana\nop verify\nend\n")
        .expect("write acl");

    ail()
        .args(["fmt", "--file"])
        .arg(acl.path())
        .arg("--check")
        .assert()
        .failure()
        .stderr(predicate::str::contains("fmt check failed"));

    ail()
        .args(["fmt", "--file"])
        .arg(acl.path())
        .arg("--write")
        .assert()
        .success();

    ail()
        .args(["fmt", "--file"])
        .arg(acl.path())
        .arg("--check")
        .assert()
        .success();
}
#[test]
fn fmt_file_json_outputs_canonical_ail_source() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let source = dir.child("main.ail");
    source
        .write_str(
            "const answer:Int=40+2\n\
fn add_pair(x:Int,y:Int)->Int=add(x,y)\n\
fn text_len(value:String)->int=len(value)\n\
fn count(values:List<Int>)->Int=len(values)\n\
fn ids()->Set<i64>=set(1,add(2,3))\n\
fn labels()->Map<String,int>=map(\"one\",1,\"two\",2)\n\
fn pair()->Tuple<i64,String>=tuple(42,\"answer\")\n\
fn person()->Record<age:i64,name:String>=record(age,42,name,\"Ada\")\n\
fn unwrap(value:Option<Int>)->Int=match(value,Some(v),v,None,0)\n\
fn main()->Int{\n\
let base:Int=answer()\n\
let values:List<Int>=[base,2+3]\n\
return if gt(base,40){add(index(values,0),2)} else {0}\n\
}\n\
test math=eq(add(sub(10,mul(2,3)),add(div(8,4),mod(7,4))),9)\n",
        )
        .expect("write source");

    let output = ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["item_count"], 11);
    let formatted = v["data"]["formatted"]
        .as_str()
        .expect("formatted must be string");
    assert!(formatted.contains("const answer: Int = 40 + 2\n"));
    assert!(formatted.contains("fn add_pair(x: Int, y: Int) -> Int = x + y\n"));
    assert!(formatted.contains("fn text_len(value: Text) -> Int = len(value)\n"));
    assert!(formatted.contains("fn count(values: List<Int>) -> Int = len(values)\n"));
    assert!(formatted.contains("fn ids() -> Set<Int> = set(1, 2 + 3)\n"));
    assert!(formatted.contains("fn labels() -> Map<Text,Int> = map(\"one\", 1, \"two\", 2)\n"));
    assert!(formatted.contains("fn pair() -> Tuple<Int,Text> = tuple(42, \"answer\")\n"));
    assert!(
        formatted
            .contains("fn person() -> Record<age:Int,name:Text> = { age: 42, name: \"Ada\" }\n")
    );
    assert!(formatted.contains("fn unwrap(value: Option<Int>) -> Int = unwrap_or(value, 0)\n"));
    assert!(formatted.contains("fn main() -> Int {\n"));
    assert!(formatted.contains("  let base: Int = answer\n"));
    assert!(formatted.contains("  let values: List<Int> = [base, 2 + 3]\n"));
    assert!(formatted.contains("  return if base > 40 { values[0] + 2 } else { 0 }\n"));
    assert!(formatted.contains("test math = 10 - 2 * 3 + (8 / 4 + 7 % 4) == 9\n"));
}
#[test]
fn fmt_ail_source_write_makes_check_pass() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let source = dir.child("main.ail");
    source
        .write_str("fn add_pair(x:Int,y:Int)->Int=add(x,y)\n")
        .expect("write source");

    ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--check")
        .assert()
        .failure()
        .stderr(predicate::str::contains("fmt check failed"));

    ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--write")
        .assert()
        .success()
        .stdout(predicate::str::contains("items: 1"));

    ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--check")
        .assert()
        .success();
}

#[test]
fn fmt_ail_source_preserves_else_if_chain() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn bucket(x:Int)->Int{\n\
return if gt(x,10){3}else if gt(x,0){2}else{1}\n\
}\n",
        )
        .expect("write source");

    let output = ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    let formatted = v["data"]["formatted"]
        .as_str()
        .expect("formatted must be string");

    assert!(formatted.contains("  return if x > 10 { 3 } else if x > 0 { 2 } else { 1 }\n"));
    assert!(
        !formatted.contains("else { if x > 0"),
        "formatter must keep else-if as first-class source syntax; got:\n{formatted}"
    );
}

#[test]
fn fmt_ail_source_renders_match_blocks_without_arm_commas() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let source = dir.child("main.ail");
    source
        .write_str("fn bucket(x:Int)->Text=match(x,0,\"zero\",1,\"one\",_,\"many\")\n")
        .expect("write source");

    let output = ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    let formatted = v["data"]["formatted"]
        .as_str()
        .expect("formatted must be string");

    assert!(
        formatted.contains(
            "fn bucket(x: Int) -> Text = match x {\n\
  0 => {\n\
    return \"zero\"\n\
  }\n\
  1 => {\n\
    return \"one\"\n\
  }\n\
  _ => {\n\
    return \"many\"\n\
  }\n\
}\n"
        ),
        "formatter must emit block match arms without commas; got:\n{formatted}"
    );
}

#[test]
fn fmt_ail_source_preserves_block_tests_with_lets() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let source = dir.child("main.ail");
    source
        .write_str("test math{\nlet actual:Int=20+22\nreturn eq(actual,42)\n}\n")
        .expect("write source");

    let output = ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    let formatted = v["data"]["formatted"]
        .as_str()
        .expect("formatted must be string");

    assert!(
        formatted.contains(
            "test math {\n\
  let actual: Int = 20 + 22\n\
  return actual == 42\n\
}\n"
        ),
        "formatter must preserve block test lets; got:\n{formatted}"
    );
}

#[test]
fn fmt_ail_source_renders_unit_literal() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let source = dir.child("main.ail");
    source
        .write_str("fn noop()->Unit=unit()\n")
        .expect("write source");

    let output = ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    let formatted = v["data"]["formatted"]
        .as_str()
        .expect("formatted must be string");

    assert!(formatted.contains("fn noop() -> Unit = ()\n"));
}
#[test]
fn fmt_stdin_json_detects_ail_source() {
    let output = ail()
        .arg("fmt")
        .arg("--json")
        .write_stdin("// source file\nfn add_pair(x:Int,y:Int)->Int=add(x,y)\n")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["item_count"], 1);
    assert_eq!(
        v["data"]["formatted"].as_str().expect("formatted string"),
        "fn add_pair(x: Int, y: Int) -> Int = x + y\n"
    );
}

#[test]
fn fmt_json_reports_unsupported_source_diagnostic() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let source = dir.child("main.ail");
    source
        .write_str("export fn helper() -> Int = 1\n")
        .expect("write source");

    let output = ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .failure()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "error");
    assert_eq!(v["data"]["code"], "FMT_UNSUPPORTED_SYNTAX");
    assert_eq!(v["data"]["category"], "unsupported");
    assert_eq!(v["data"]["diagnostic"]["code"], "FMT_UNSUPPORTED_SYNTAX");
    assert_eq!(v["data"]["descriptor"]["input"], "file");
    assert_eq!(v["data"]["descriptor"]["extension"], "ail");
    assert_eq!(v["data"]["descriptor"]["language"], "ail-source");
}

#[test]
fn fmt_json_rejects_check_write_mode_mismatch() {
    let output = ail()
        .arg("fmt")
        .arg("--check")
        .arg("--write")
        .arg("--json")
        .assert()
        .failure()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "error");
    assert_eq!(v["data"]["code"], "FMT_WRITE_CHECK_MODE_MISMATCH");
    assert_eq!(v["data"]["category"], "usage");
    assert_eq!(v["data"]["descriptor"]["mode"], "check-write");
    assert_eq!(v["data"]["descriptor"]["input"], "stdin");
}

#[test]
fn fmt_json_rejects_write_without_file() {
    let output = ail()
        .arg("fmt")
        .arg("--write")
        .arg("--json")
        .write_stdin("fn add_pair(x:Int,y:Int)->Int=add(x,y)\n")
        .assert()
        .failure()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "error");
    assert_eq!(v["data"]["code"], "FMT_WRITE_REQUIRES_FILE");
    assert_eq!(v["data"]["category"], "usage");
    assert_eq!(v["data"]["descriptor"]["mode"], "write");
    assert_eq!(v["data"]["descriptor"]["input"], "stdin");
    assert_eq!(v["data"]["descriptor"]["extension"], "none");
}

// ── ail link integration tests ─────────────────────────────────────────────
