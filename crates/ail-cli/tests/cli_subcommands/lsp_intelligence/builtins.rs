// Mechanical phase 2 split from lsp_intelligence.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;

#[test]
fn lsp_completion_and_hover_cover_ail_source_builtins() {
    let completion_output = ail()
        .args(["lsp", "--complete", "effect", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let completion = parse_json_output(&completion_output);
    assert_eq!(completion["status"], "ok");
    let items = completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        items.iter().any(|item| item["label"] == "effect_call"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("effect_call(${1:log.write}")),
        "completion must include AIL source effect_call snippet; got: {items:?}"
    );

    let print_completion_output = ail()
        .args(["lsp", "--complete", "log", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let print_completion = parse_json_output(&print_completion_output);
    let print_items = print_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        print_items
            .iter()
            .any(|item| item["label"] == "log_write" && item["detail"] == "AIL source log effect"),
        "completion must include AIL source log_write helper; got: {print_items:?}"
    );

    let hover_output = ail()
        .args(["lsp", "--hover-token", "effect_call", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let hover = parse_json_output(&hover_output);
    assert_eq!(hover["status"], "ok");
    assert!(
        hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("explicit grant"),
        "hover must explain effect_call grants; got: {hover}"
    );

    let print_hover_output = ail()
        .args(["lsp", "--hover-token", "log_write", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let print_hover = parse_json_output(&print_hover_output);
    assert!(
        print_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("requires capability log.write"),
        "hover must explain log_write grants; got: {print_hover}"
    );

    let first_or_completion_output = ail()
        .args(["lsp", "--complete", "first", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let first_or_completion = parse_json_output(&first_or_completion_output);
    let first_or_items = first_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        first_or_items
            .iter()
            .any(|item| item["label"] == "first_or" && item["detail"] == "AIL source List helper"),
        "completion must include AIL source first_or helper; got: {first_or_items:?}"
    );

    let last_or_completion_output = ail()
        .args(["lsp", "--complete", "last", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let last_or_completion = parse_json_output(&last_or_completion_output);
    let last_or_items = last_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        last_or_items
            .iter()
            .any(|item| item["label"] == "last_or" && item["detail"] == "AIL source List helper"),
        "completion must include AIL source last_or helper; got: {last_or_items:?}"
    );

    let get_or_completion_output = ail()
        .args(["lsp", "--complete", "get", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let get_or_completion = parse_json_output(&get_or_completion_output);
    let get_or_items = get_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        get_or_items
            .iter()
            .any(|item| item["label"] == "get_or" && item["detail"] == "AIL source List helper"),
        "completion must include AIL source get_or helper; got: {get_or_items:?}"
    );

    let list_get_completion_output = ail()
        .args(["lsp", "--complete", "list_get", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let list_get_completion = parse_json_output(&list_get_completion_output);
    let list_get_items = list_get_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        list_get_items
            .iter()
            .any(|item| item["label"] == "list_get" && item["detail"] == "AIL source List helper"),
        "completion must include AIL source list_get helper; got: {list_get_items:?}"
    );

    let length_completion_output = ail()
        .args(["lsp", "--complete", "length", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let length_completion = parse_json_output(&length_completion_output);
    let length_items = length_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        length_items.iter().any(
            |item| item["label"] == "text_length" && item["detail"] == "AIL source Text helper"
        ) && length_items.iter().any(
            |item| item["label"] == "list_length" && item["detail"] == "AIL source List helper"
        ),
        "completion must include AIL source length helpers; got: {length_items:?}"
    );

    let is_empty_completion_output = ail()
        .args(["lsp", "--complete", "is_empty", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let is_empty_completion = parse_json_output(&is_empty_completion_output);
    let is_empty_items = is_empty_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        is_empty_items
            .iter()
            .any(|item| item["label"] == "is_empty"
                && item["detail"] == "AIL source sized predicate")
            && is_empty_items
                .iter()
                .any(|item| item["label"] == "list_is_empty"
                    && item["detail"] == "AIL source List predicate")
            && is_empty_items
                .iter()
                .any(|item| item["label"] == "text_is_empty"
                    && item["detail"] == "AIL source Text predicate"),
        "completion must include AIL source is_empty helper; got: {is_empty_items:?}"
    );

    let ok_or_completion_output = ail()
        .args(["lsp", "--complete", "ok_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let ok_or_completion = parse_json_output(&ok_or_completion_output);
    let ok_or_items = ok_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ok_or_items
            .iter()
            .any(|item| item["label"] == "ok_or" && item["detail"] == "AIL source Option helper")
            && ok_or_items
                .iter()
                .any(|item| item["label"] == "option_ok_or"
                    && item["detail"] == "AIL source Option helper")
            && ok_or_items
                .iter()
                .any(|item| item["label"] == "option.ok_or"
                    && item["detail"] == "AIL source Option helper"),
        "completion must include AIL source ok_or helpers; got: {ok_or_items:?}"
    );

    let result_unwrap_completion_output = ail()
        .args(["lsp", "--complete", "result_unwrap", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let result_unwrap_completion = parse_json_output(&result_unwrap_completion_output);
    let result_unwrap_items = result_unwrap_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        result_unwrap_items
            .iter()
            .any(|item| item["label"] == "result_unwrap_or"
                && item["detail"] == "AIL source Result helper"),
        "completion must include AIL source result_unwrap_or helper; got: {result_unwrap_items:?}"
    );

    let dotted_result_completion_output = ail()
        .args(["lsp", "--complete", "result.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_result_completion = parse_json_output(&dotted_result_completion_output);
    let dotted_result_items = dotted_result_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        dotted_result_items
            .iter()
            .any(|item| item["label"] == "result.unwrap_or"
                && item["detail"] == "AIL source Result helper")
            && dotted_result_items
                .iter()
                .any(|item| item["label"] == "result.is_ok"
                    && item["detail"] == "AIL source Result predicate")
            && dotted_result_items
                .iter()
                .any(|item| item["label"] == "result.is_err"
                    && item["detail"] == "AIL source Result predicate"),
        "completion must include dotted Result helpers; got: {dotted_result_items:?}"
    );

    let list_push_completion_output = ail()
        .args(["lsp", "--complete", "list_push", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let list_push_completion = parse_json_output(&list_push_completion_output);
    let list_push_items = list_push_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        list_push_items
            .iter()
            .any(|item| item["label"] == "list_push" && item["detail"] == "AIL source List helper"),
        "completion must include AIL source list_push helper; got: {list_push_items:?}"
    );

    let list_concat_completion_output = ail()
        .args(["lsp", "--complete", "list_concat", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let list_concat_completion = parse_json_output(&list_concat_completion_output);
    let list_concat_items = list_concat_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        list_concat_items.iter().any(
            |item| item["label"] == "list_concat" && item["detail"] == "AIL source List helper"
        ),
        "completion must include AIL source list_concat helper; got: {list_concat_items:?}"
    );

    let queue_push_completion_output = ail()
        .args(["lsp", "--complete", "queue_push", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let queue_push_completion = parse_json_output(&queue_push_completion_output);
    let queue_push_items = queue_push_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        queue_push_items
            .iter()
            .any(|item| item["label"] == "queue_push_back"
                && item["detail"] == "AIL source Queue helper"),
        "completion must include AIL source queue_push_back helper; got: {queue_push_items:?}"
    );

    let queue_pop_completion_output = ail()
        .args(["lsp", "--complete", "queue_pop", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let queue_pop_completion = parse_json_output(&queue_pop_completion_output);
    let queue_pop_items = queue_pop_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        queue_pop_items
            .iter()
            .any(|item| item["label"] == "queue_pop_front"
                && item["detail"] == "AIL source Queue helper"),
        "completion must include AIL source queue_pop_front helper; got: {queue_pop_items:?}"
    );

    let set_contains_completion_output = ail()
        .args(["lsp", "--complete", "set_contains", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let set_contains_completion = parse_json_output(&set_contains_completion_output);
    let set_contains_items = set_contains_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        set_contains_items.iter().any(
            |item| item["label"] == "set_contains" && item["detail"] == "AIL source Set helper"
        ),
        "completion must include AIL source set_contains helper; got: {set_contains_items:?}"
    );

    let set_insert_completion_output = ail()
        .args(["lsp", "--complete", "set_insert", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let set_insert_completion = parse_json_output(&set_insert_completion_output);
    let set_insert_items = set_insert_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        set_insert_items
            .iter()
            .any(|item| item["label"] == "set_insert" && item["detail"] == "AIL source Set helper"),
        "completion must include AIL source set_insert helper; got: {set_insert_items:?}"
    );

    let map_get_completion_output = ail()
        .args(["lsp", "--complete", "map_get", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let map_get_completion = parse_json_output(&map_get_completion_output);
    let map_get_items = map_get_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        map_get_items
            .iter()
            .any(|item| item["label"] == "map_get" && item["detail"] == "AIL source Map helper"),
        "completion must include AIL source map_get helper; got: {map_get_items:?}"
    );

    let map_insert_completion_output = ail()
        .args(["lsp", "--complete", "map_insert", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let map_insert_completion = parse_json_output(&map_insert_completion_output);
    let map_insert_items = map_insert_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        map_insert_items
            .iter()
            .any(|item| item["label"] == "map_insert" && item["detail"] == "AIL source Map helper"),
        "completion must include AIL source map_insert helper; got: {map_insert_items:?}"
    );

    let text_eq_completion_output = ail()
        .args(["lsp", "--complete", "text_eq", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_eq_completion = parse_json_output(&text_eq_completion_output);
    let text_eq_items = text_eq_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_eq_items.iter().any(|item| item["label"] == "text_eq"
            && item["detail"] == "AIL source Text predicate"),
        "completion must include AIL source text_eq helper; got: {text_eq_items:?}"
    );

    let text_trim_completion_output = ail()
        .args(["lsp", "--complete", "text_trim", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_trim_completion = parse_json_output(&text_trim_completion_output);
    let text_trim_items = text_trim_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_trim_items
            .iter()
            .any(|item| item["label"] == "text_trim" && item["detail"] == "AIL source Text helper"),
        "completion must include AIL source text_trim helper; got: {text_trim_items:?}"
    );

    let int_clamp_completion_output = ail()
        .args(["lsp", "--complete", "int_clamp", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_clamp_completion = parse_json_output(&int_clamp_completion_output);
    let int_clamp_items = int_clamp_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_clamp_items
            .iter()
            .any(|item| item["label"] == "int_clamp"
                && item["detail"] == "AIL source Int bounds helper"),
        "completion must include AIL source int_clamp helper; got: {int_clamp_items:?}"
    );

    let int_abs_or_completion_output = ail()
        .args(["lsp", "--complete", "int_abs_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_abs_or_completion = parse_json_output(&int_abs_or_completion_output);
    let int_abs_or_items = int_abs_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_abs_or_items
            .iter()
            .any(|item| item["label"] == "int_abs_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_abs_or helper; got: {int_abs_or_items:?}"
    );

    let int_neg_or_completion_output = ail()
        .args(["lsp", "--complete", "int_neg_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_neg_or_completion = parse_json_output(&int_neg_or_completion_output);
    let int_neg_or_items = int_neg_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_neg_or_items
            .iter()
            .any(|item| item["label"] == "int_neg_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_neg_or helper; got: {int_neg_or_items:?}"
    );

    let int_add_or_completion_output = ail()
        .args(["lsp", "--complete", "int_add_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_add_or_completion = parse_json_output(&int_add_or_completion_output);
    let int_add_or_items = int_add_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_add_or_items
            .iter()
            .any(|item| item["label"] == "int_add_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_add_or helper; got: {int_add_or_items:?}"
    );

    let int_sub_or_completion_output = ail()
        .args(["lsp", "--complete", "int_sub_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_sub_or_completion = parse_json_output(&int_sub_or_completion_output);
    let int_sub_or_items = int_sub_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_sub_or_items
            .iter()
            .any(|item| item["label"] == "int_sub_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_sub_or helper; got: {int_sub_or_items:?}"
    );

    let int_mul_or_completion_output = ail()
        .args(["lsp", "--complete", "int_mul_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_mul_or_completion = parse_json_output(&int_mul_or_completion_output);
    let int_mul_or_items = int_mul_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_mul_or_items
            .iter()
            .any(|item| item["label"] == "int_mul_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_mul_or helper; got: {int_mul_or_items:?}"
    );

    let int_saturating_add_completion_output = ail()
        .args(["lsp", "--complete", "int_saturating_add", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_saturating_add_completion = parse_json_output(&int_saturating_add_completion_output);
    let int_saturating_add_items = int_saturating_add_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_saturating_add_items
            .iter()
            .any(|item| item["label"] == "int_saturating_add"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_saturating_add helper; got: {int_saturating_add_items:?}"
    );

    let int_saturating_sub_completion_output = ail()
        .args(["lsp", "--complete", "int_saturating_sub", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_saturating_sub_completion = parse_json_output(&int_saturating_sub_completion_output);
    let int_saturating_sub_items = int_saturating_sub_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_saturating_sub_items
            .iter()
            .any(|item| item["label"] == "int_saturating_sub"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_saturating_sub helper; got: {int_saturating_sub_items:?}"
    );

    let int_saturating_mul_completion_output = ail()
        .args(["lsp", "--complete", "int_saturating_mul", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_saturating_mul_completion = parse_json_output(&int_saturating_mul_completion_output);
    let int_saturating_mul_items = int_saturating_mul_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_saturating_mul_items
            .iter()
            .any(|item| item["label"] == "int_saturating_mul"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_saturating_mul helper; got: {int_saturating_mul_items:?}"
    );

    let int_saturating_neg_completion_output = ail()
        .args(["lsp", "--complete", "int_saturating_neg", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_saturating_neg_completion = parse_json_output(&int_saturating_neg_completion_output);
    let int_saturating_neg_items = int_saturating_neg_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_saturating_neg_items
            .iter()
            .any(|item| item["label"] == "int_saturating_neg"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_saturating_neg helper; got: {int_saturating_neg_items:?}"
    );

    let int_wrapping_add_completion_output = ail()
        .args(["lsp", "--complete", "int_wrapping_add", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_wrapping_add_completion = parse_json_output(&int_wrapping_add_completion_output);
    let int_wrapping_add_items = int_wrapping_add_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_wrapping_add_items
            .iter()
            .any(|item| item["label"] == "int_wrapping_add"
                && item["detail"] == "AIL source Int explicit wrapping helper"),
        "completion must include AIL source int_wrapping_add helper; got: {int_wrapping_add_items:?}"
    );

    let int_wrapping_sub_completion_output = ail()
        .args(["lsp", "--complete", "int_wrapping_sub", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_wrapping_sub_completion = parse_json_output(&int_wrapping_sub_completion_output);
    let int_wrapping_sub_items = int_wrapping_sub_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_wrapping_sub_items
            .iter()
            .any(|item| item["label"] == "int_wrapping_sub"
                && item["detail"] == "AIL source Int explicit wrapping helper"),
        "completion must include AIL source int_wrapping_sub helper; got: {int_wrapping_sub_items:?}"
    );

    let int_wrapping_mul_completion_output = ail()
        .args(["lsp", "--complete", "int_wrapping_mul", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_wrapping_mul_completion = parse_json_output(&int_wrapping_mul_completion_output);
    let int_wrapping_mul_items = int_wrapping_mul_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_wrapping_mul_items
            .iter()
            .any(|item| item["label"] == "int_wrapping_mul"
                && item["detail"] == "AIL source Int explicit wrapping helper"),
        "completion must include AIL source int_wrapping_mul helper; got: {int_wrapping_mul_items:?}"
    );

    let int_wrapping_neg_completion_output = ail()
        .args(["lsp", "--complete", "int_wrapping_neg", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_wrapping_neg_completion = parse_json_output(&int_wrapping_neg_completion_output);
    let int_wrapping_neg_items = int_wrapping_neg_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_wrapping_neg_items
            .iter()
            .any(|item| item["label"] == "int_wrapping_neg"
                && item["detail"] == "AIL source Int explicit wrapping helper"),
        "completion must include AIL source int_wrapping_neg helper; got: {int_wrapping_neg_items:?}"
    );

    let int_bit_and_completion_output = ail()
        .args(["lsp", "--complete", "int_bit_and", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_bit_and_completion = parse_json_output(&int_bit_and_completion_output);
    let int_bit_and_items = int_bit_and_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_bit_and_items
            .iter()
            .any(|item| item["label"] == "int_bit_and"
                && item["detail"] == "AIL source Int bitwise helper"),
        "completion must include AIL source int_bit_and helper; got: {int_bit_and_items:?}"
    );

    let int_bit_or_completion_output = ail()
        .args(["lsp", "--complete", "int_bit_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_bit_or_completion = parse_json_output(&int_bit_or_completion_output);
    let int_bit_or_items = int_bit_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_bit_or_items
            .iter()
            .any(|item| item["label"] == "int_bit_or"
                && item["detail"] == "AIL source Int bitwise helper"),
        "completion must include AIL source int_bit_or helper; got: {int_bit_or_items:?}"
    );

    let int_bit_xor_completion_output = ail()
        .args(["lsp", "--complete", "int_bit_xor", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_bit_xor_completion = parse_json_output(&int_bit_xor_completion_output);
    let int_bit_xor_items = int_bit_xor_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_bit_xor_items
            .iter()
            .any(|item| item["label"] == "int_bit_xor"
                && item["detail"] == "AIL source Int bitwise helper"),
        "completion must include AIL source int_bit_xor helper; got: {int_bit_xor_items:?}"
    );

    let int_bit_not_completion_output = ail()
        .args(["lsp", "--complete", "int_bit_not", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_bit_not_completion = parse_json_output(&int_bit_not_completion_output);
    let int_bit_not_items = int_bit_not_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_bit_not_items
            .iter()
            .any(|item| item["label"] == "int_bit_not"
                && item["detail"] == "AIL source Int bitwise helper"),
        "completion must include AIL source int_bit_not helper; got: {int_bit_not_items:?}"
    );

    let int_shift_left_completion_output = ail()
        .args(["lsp", "--complete", "int_shift_left", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_shift_left_completion = parse_json_output(&int_shift_left_completion_output);
    let int_shift_left_items = int_shift_left_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_shift_left_items
            .iter()
            .any(|item| item["label"] == "int_shift_left"
                && item["detail"] == "AIL source Int bit shift helper"),
        "completion must include AIL source int_shift_left helper; got: {int_shift_left_items:?}"
    );

    let int_shift_right_completion_output = ail()
        .args(["lsp", "--complete", "int_shift_right", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_shift_right_completion = parse_json_output(&int_shift_right_completion_output);
    let int_shift_right_items = int_shift_right_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_shift_right_items
            .iter()
            .any(|item| item["label"] == "int_shift_right"
                && item["detail"] == "AIL source Int bit shift helper"),
        "completion must include AIL source int_shift_right helper; got: {int_shift_right_items:?}"
    );

    let int_shift_right_unsigned_completion_output = ail()
        .args(["lsp", "--complete", "int_shift_right_unsigned", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_shift_right_unsigned_completion =
        parse_json_output(&int_shift_right_unsigned_completion_output);
    let int_shift_right_unsigned_items = int_shift_right_unsigned_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_shift_right_unsigned_items
            .iter()
            .any(|item| item["label"] == "int_shift_right_unsigned"
                && item["detail"] == "AIL source Int bit shift helper"),
        "completion must include AIL source int_shift_right_unsigned helper; got: {int_shift_right_unsigned_items:?}"
    );

    let int_div_or_completion_output = ail()
        .args(["lsp", "--complete", "int_div_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_div_or_completion = parse_json_output(&int_div_or_completion_output);
    let int_div_or_items = int_div_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_div_or_items
            .iter()
            .any(|item| item["label"] == "int_div_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_div_or helper; got: {int_div_or_items:?}"
    );

    let int_rem_or_completion_output = ail()
        .args(["lsp", "--complete", "int_rem_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let int_rem_or_completion = parse_json_output(&int_rem_or_completion_output);
    let int_rem_or_items = int_rem_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        int_rem_or_items
            .iter()
            .any(|item| item["label"] == "int_rem_or"
                && item["detail"] == "AIL source Int safety helper"),
        "completion must include AIL source int_rem_or helper; got: {int_rem_or_items:?}"
    );

    let text_contains_completion_output = ail()
        .args(["lsp", "--complete", "text_contains", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_contains_completion = parse_json_output(&text_contains_completion_output);
    let text_contains_items = text_contains_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_contains_items
            .iter()
            .any(|item| item["label"] == "text_contains"
                && item["detail"] == "AIL source Text predicate"),
        "completion must include AIL source text_contains helper; got: {text_contains_items:?}"
    );

    let text_index_of_completion_output = ail()
        .args(["lsp", "--complete", "text_index_of", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_index_of_completion = parse_json_output(&text_index_of_completion_output);
    let text_index_of_items = text_index_of_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_index_of_items
            .iter()
            .any(|item| item["label"] == "text_index_of"
                && item["detail"] == "AIL source Text search"),
        "completion must include AIL source text_index_of helper; got: {text_index_of_items:?}"
    );

    let text_parse_int_or_completion_output = ail()
        .args(["lsp", "--complete", "text_parse_int_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_parse_int_or_completion = parse_json_output(&text_parse_int_or_completion_output);
    let text_parse_int_or_items = text_parse_int_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_parse_int_or_items
            .iter()
            .any(|item| item["label"] == "text_parse_int_or"
                && item["detail"] == "AIL source Text parser"),
        "completion must include AIL source text_parse_int_or helper; got: {text_parse_int_or_items:?}"
    );

    let text_byte_at_or_completion_output = ail()
        .args(["lsp", "--complete", "text_byte_at_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_byte_at_or_completion = parse_json_output(&text_byte_at_or_completion_output);
    let text_byte_at_or_items = text_byte_at_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_byte_at_or_items
            .iter()
            .any(|item| item["label"] == "text_byte_at_or"
                && item["detail"] == "AIL source Text helper"),
        "completion must include AIL source text_byte_at_or helper; got: {text_byte_at_or_items:?}"
    );

    let text_slice_completion_output = ail()
        .args(["lsp", "--complete", "text_slice", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_slice_completion = parse_json_output(&text_slice_completion_output);
    let text_slice_items = text_slice_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_slice_items
            .iter()
            .any(|item| item["label"] == "text_slice"
                && item["detail"] == "AIL source Text helper"),
        "completion must include AIL source text_slice helper; got: {text_slice_items:?}"
    );

    let text_replace_first_completion_output = ail()
        .args(["lsp", "--complete", "text_replace_first", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_replace_first_completion = parse_json_output(&text_replace_first_completion_output);
    let text_replace_first_items = text_replace_first_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_replace_first_items
            .iter()
            .any(|item| item["label"] == "text_replace_first"
                && item["detail"] == "AIL source Text helper"),
        "completion must include AIL source text_replace_first helper; got: {text_replace_first_items:?}"
    );

    let text_starts_with_completion_output = ail()
        .args(["lsp", "--complete", "text_starts_with", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_starts_with_completion = parse_json_output(&text_starts_with_completion_output);
    let text_starts_with_items = text_starts_with_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_starts_with_items
            .iter()
            .any(|item| item["label"] == "text_starts_with"
                && item["detail"] == "AIL source Text predicate"),
        "completion must include AIL source text_starts_with helper; got: {text_starts_with_items:?}"
    );

    let text_ends_with_completion_output = ail()
        .args(["lsp", "--complete", "text_ends_with", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text_ends_with_completion = parse_json_output(&text_ends_with_completion_output);
    let text_ends_with_items = text_ends_with_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        text_ends_with_items
            .iter()
            .any(|item| item["label"] == "text_ends_with"
                && item["detail"] == "AIL source Text predicate"),
        "completion must include AIL source text_ends_with helper; got: {text_ends_with_items:?}"
    );

    let map_completion_output = ail()
        .args(["lsp", "--complete", "ma", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let map_completion = parse_json_output(&map_completion_output);
    let map_items = map_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        map_items
            .iter()
            .any(|item| item["label"] == "map" && item["detail"] == "AIL source Map builtin"),
        "completion must include AIL source map builtin; got: {map_items:?}"
    );

    let set_hover_output = ail()
        .args(["lsp", "--hover-token", "set", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let set_hover = parse_json_output(&set_hover_output);
    assert!(
        set_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("Set<T>"),
        "hover must explain source Set builtin; got: {set_hover}"
    );

    let tuple_completion_output = ail()
        .args(["lsp", "--complete", "tu", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let tuple_completion = parse_json_output(&tuple_completion_output);
    let tuple_items = tuple_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        tuple_items
            .iter()
            .any(|item| item["label"] == "tuple" && item["detail"] == "AIL source Tuple builtin"),
        "completion must include AIL source tuple builtin; got: {tuple_items:?}"
    );
    assert!(
        tuple_items.iter().any(|item| {
            item["label"] == "tuple_first" && item["detail"] == "AIL source Tuple helper"
        }),
        "completion must include AIL source tuple helper; got: {tuple_items:?}"
    );

    let record_completion_output = ail()
        .args(["lsp", "--complete", "rec", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let record_completion = parse_json_output(&record_completion_output);
    let record_items = record_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        record_items
            .iter()
            .any(|item| item["label"] == "record" && item["detail"] == "AIL source Record builtin"),
        "completion must include AIL source record builtin; got: {record_items:?}"
    );

    let option_completion_output = ail()
        .args(["lsp", "--complete", "Som", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let option_completion = parse_json_output(&option_completion_output);
    let option_items = option_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        option_items.iter().any(
            |item| item["label"] == "Some" && item["detail"] == "AIL source Option constructor"
        ),
        "completion must include AIL source Option constructor; got: {option_items:?}"
    );

    let result_hover_output = ail()
        .args(["lsp", "--hover-token", "Err", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let result_hover = parse_json_output(&result_hover_output);
    assert!(
        result_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("Result<T,E> error"),
        "hover must explain source Result constructor; got: {result_hover}"
    );

    let unwrap_or_completion_output = ail()
        .args(["lsp", "--complete", "unwrap", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let unwrap_or_completion = parse_json_output(&unwrap_or_completion_output);
    let unwrap_or_items = unwrap_or_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        unwrap_or_items.iter().any(
            |item| item["label"] == "unwrap_or" && item["detail"] == "AIL source Option helper"
        ) && unwrap_or_items
            .iter()
            .any(|item| item["label"] == "option_unwrap_or"
                && item["detail"] == "AIL source Option helper"),
        "completion must include AIL source unwrap_or helpers; got: {unwrap_or_items:?}"
    );

    let option_fallback_completion_output = ail()
        .args(["lsp", "--complete", "option_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let option_fallback_completion = parse_json_output(&option_fallback_completion_output);
    let option_fallback_items = option_fallback_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        option_fallback_items
            .iter()
            .any(|item| item["label"] == "option_unwrap_or"
                && item["detail"] == "AIL source Option helper")
            && option_fallback_items
                .iter()
                .any(|item| item["label"] == "option_ok_or"
                    && item["detail"] == "AIL source Option helper"),
        "completion must include namespaced Option fallback helpers; got: {option_fallback_items:?}"
    );

    let dotted_option_completion_output = ail()
        .args(["lsp", "--complete", "option.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_option_completion = parse_json_output(&dotted_option_completion_output);
    let dotted_option_items = dotted_option_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        dotted_option_items
            .iter()
            .any(|item| item["label"] == "option.unwrap_or"
                && item["detail"] == "AIL source Option helper")
            && dotted_option_items
                .iter()
                .any(|item| item["label"] == "option.ok_or"
                    && item["detail"] == "AIL source Option helper")
            && dotted_option_items
                .iter()
                .any(|item| item["label"] == "option.is_some"
                    && item["detail"] == "AIL source Option predicate")
            && dotted_option_items
                .iter()
                .any(|item| item["label"] == "option.is_none"
                    && item["detail"] == "AIL source Option predicate"),
        "completion must include dotted Option helpers; got: {dotted_option_items:?}"
    );

    let option_unwrap_hover_output = ail()
        .args(["lsp", "--hover-token", "option_unwrap_or", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let option_unwrap_hover = parse_json_output(&option_unwrap_hover_output);
    assert!(
        option_unwrap_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("Namespaced alias"),
        "hover must explain source option_unwrap_or helper; got: {option_unwrap_hover}"
    );

    let option_predicate_completion_output = ail()
        .args(["lsp", "--complete", "is_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let option_predicate_completion = parse_json_output(&option_predicate_completion_output);
    let option_predicate_items = option_predicate_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        option_predicate_items
            .iter()
            .any(|item| item["label"] == "is_some"
                && item["detail"] == "AIL source Option predicate")
            && option_predicate_items
                .iter()
                .any(|item| item["label"] == "is_none"
                    && item["detail"] == "AIL source Option predicate")
            && option_predicate_items
                .iter()
                .any(|item| item["label"] == "option_is_some"
                    && item["detail"] == "AIL source Option predicate")
            && option_predicate_items
                .iter()
                .any(|item| item["label"] == "option_is_none"
                    && item["detail"] == "AIL source Option predicate"),
        "completion must include AIL source Option predicates; got: {option_predicate_items:?}"
    );

    let result_predicate_completion_output = ail()
        .args(["lsp", "--complete", "is_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let result_predicate_completion = parse_json_output(&result_predicate_completion_output);
    let result_predicate_items = result_predicate_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        result_predicate_items
            .iter()
            .any(|item| item["label"] == "is_ok" && item["detail"] == "AIL source Result predicate")
            && result_predicate_items
                .iter()
                .any(|item| item["label"] == "is_err"
                    && item["detail"] == "AIL source Result predicate")
            && result_predicate_items
                .iter()
                .any(|item| item["label"] == "result_is_ok"
                    && item["detail"] == "AIL source Result predicate")
            && result_predicate_items
                .iter()
                .any(|item| item["label"] == "result_is_err"
                    && item["detail"] == "AIL source Result predicate"),
        "completion must include AIL source Result predicates; got: {result_predicate_items:?}"
    );
}
