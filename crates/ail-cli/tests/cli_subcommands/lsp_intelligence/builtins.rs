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
            .any(|item| item["label"] == "log_write" && item["detail"] == "AIL source log effect")
            && print_items
                .iter()
                .any(|item| item["label"] == "log.write"
                    && item["detail"] == "AIL source log effect"),
        "completion must include AIL source log helpers; got: {print_items:?}"
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

    let dotted_list_completion_output = ail()
        .args(["lsp", "--complete", "list.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_list_completion = parse_json_output(&dotted_list_completion_output);
    let dotted_list_items = dotted_list_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["list.get", "list.push", "list.concat", "list.length"]
            .iter()
            .all(|label| dotted_list_items
                .iter()
                .any(|item| item["label"] == *label && item["detail"] == "AIL source List helper"))
            && dotted_list_items
                .iter()
                .any(|item| item["label"] == "list.is_empty"
                    && item["detail"] == "AIL source List predicate"),
        "completion must include dotted List helpers; got: {dotted_list_items:?}"
    );

    let dotted_queue_completion_output = ail()
        .args(["lsp", "--complete", "queue.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_queue_completion = parse_json_output(&dotted_queue_completion_output);
    let dotted_queue_items = dotted_queue_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        [
            "queue.push_back",
            "queue.pop_front",
            "queue.peek_front",
            "queue.length",
            "queue.is_empty",
        ]
        .iter()
        .all(|label| dotted_queue_items
            .iter()
            .any(|item| item["label"] == *label && item["detail"] == "AIL source Queue helper")),
        "completion must include dotted Queue helpers; got: {dotted_queue_items:?}"
    );

    let dotted_set_completion_output = ail()
        .args(["lsp", "--complete", "set.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_set_completion = parse_json_output(&dotted_set_completion_output);
    let dotted_set_items = dotted_set_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["set.contains", "set.length", "set.insert"]
            .iter()
            .all(|label| dotted_set_items
                .iter()
                .any(|item| item["label"] == *label && item["detail"] == "AIL source Set helper")),
        "completion must include dotted Set helpers; got: {dotted_set_items:?}"
    );

    let dotted_map_completion_output = ail()
        .args(["lsp", "--complete", "map.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_map_completion = parse_json_output(&dotted_map_completion_output);
    let dotted_map_items = dotted_map_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["map.get", "map.contains_key", "map.length", "map.insert"]
            .iter()
            .all(|label| dotted_map_items
                .iter()
                .any(|item| item["label"] == *label && item["detail"] == "AIL source Map helper")),
        "completion must include dotted Map helpers; got: {dotted_map_items:?}"
    );

    let crypto_completion_output = ail()
        .args(["lsp", "--complete", "crypto_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let crypto_completion = parse_json_output(&crypto_completion_output);
    let crypto_items = crypto_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["crypto_hash", "crypto_hmac", "crypto_constant_time_eq"]
            .iter()
            .all(|label| crypto_items.iter().any(
                |item| item["label"] == *label && item["detail"] == "AIL source Crypto helper"
            )),
        "completion must include AIL source crypto helpers; got: {crypto_items:?}"
    );

    let dotted_crypto_completion_output = ail()
        .args(["lsp", "--complete", "crypto.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_crypto_completion = parse_json_output(&dotted_crypto_completion_output);
    let dotted_crypto_items = dotted_crypto_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["crypto.hash", "crypto.hmac", "crypto.constant_time_eq"]
            .iter()
            .all(|label| dotted_crypto_items.iter().any(
                |item| item["label"] == *label && item["detail"] == "AIL source Crypto helper"
            )),
        "completion must include dotted Crypto helpers; got: {dotted_crypto_items:?}"
    );

    let crypto_hover_output = ail()
        .args(["lsp", "--hover-token", "crypto_constant_time_eq", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let crypto_hover = parse_json_output(&crypto_hover_output);
    assert!(
        crypto_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("without early-exit timing leaks"),
        "hover must explain crypto_constant_time_eq; got: {crypto_hover}"
    );

    let json_completion_output = ail()
        .args(["lsp", "--complete", "json_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let json_completion = parse_json_output(&json_completion_output);
    let json_items = json_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["json_parse", "json_stringify"]
            .iter()
            .all(|label| json_items
                .iter()
                .any(|item| item["label"] == *label && item["detail"] == "AIL source JSON helper")),
        "completion must include AIL source JSON helpers; got: {json_items:?}"
    );

    let dotted_json_completion_output = ail()
        .args(["lsp", "--complete", "json.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_json_completion = parse_json_output(&dotted_json_completion_output);
    let dotted_json_items = dotted_json_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["json.parse", "json.stringify"]
            .iter()
            .all(|label| dotted_json_items
                .iter()
                .any(|item| item["label"] == *label && item["detail"] == "AIL source JSON helper")),
        "completion must include dotted JSON helpers; got: {dotted_json_items:?}"
    );

    let json_hover_output = ail()
        .args(["lsp", "--hover-token", "json_parse", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let json_hover = parse_json_output(&json_hover_output);
    assert!(
        json_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("Result<Json,Text>"),
        "hover must explain json_parse; got: {json_hover}"
    );

    let path_completion_output = ail()
        .args(["lsp", "--complete", "path_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let path_completion = parse_json_output(&path_completion_output);
    let path_items = path_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["path_from_text", "path_to_text"]
            .iter()
            .all(|label| path_items
                .iter()
                .any(|item| item["label"] == *label && item["detail"] == "AIL source Path helper")),
        "completion must include AIL source Path helpers; got: {path_items:?}"
    );

    let dotted_path_completion_output = ail()
        .args(["lsp", "--complete", "path.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_path_completion = parse_json_output(&dotted_path_completion_output);
    let dotted_path_items = dotted_path_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["path.from_text", "path.to_text"]
            .iter()
            .all(|label| dotted_path_items
                .iter()
                .any(|item| item["label"] == *label && item["detail"] == "AIL source Path helper")),
        "completion must include dotted Path helpers; got: {dotted_path_items:?}"
    );

    let path_hover_output = ail()
        .args(["lsp", "--hover-token", "path_from_text", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let path_hover = parse_json_output(&path_hover_output);
    assert!(
        path_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("runtime Path value"),
        "hover must explain path_from_text; got: {path_hover}"
    );

    let env_completion_output = ail()
        .args(["lsp", "--complete", "env_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let env_completion = parse_json_output(&env_completion_output);
    let env_items = env_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["env_get", "env_set", "env_list"]
            .iter()
            .all(|label| env_items
                .iter()
                .any(|item| item["label"] == *label && item["detail"] == "AIL source Env helper")),
        "completion must include AIL source Env helpers; got: {env_items:?}"
    );

    let dotted_env_completion_output = ail()
        .args(["lsp", "--complete", "env.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_env_completion = parse_json_output(&dotted_env_completion_output);
    let dotted_env_items = dotted_env_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["env.get", "env.set", "env.list"]
            .iter()
            .all(|label| dotted_env_items
                .iter()
                .any(|item| item["label"] == *label && item["detail"] == "AIL source Env helper")),
        "completion must include dotted Env helpers; got: {dotted_env_items:?}"
    );

    let env_hover_output = ail()
        .args(["lsp", "--hover-token", "env_get", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let env_hover = parse_json_output(&env_hover_output);
    assert!(
        env_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("requires an explicit grant"),
        "hover must explain env_get; got: {env_hover}"
    );

    let fs_completion_output = ail()
        .args(["lsp", "--complete", "fs_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let fs_completion = parse_json_output(&fs_completion_output);
    let fs_items = fs_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["fs_read_file", "fs_write", "fs_delete", "fs_list"]
            .iter()
            .all(|label| fs_items
                .iter()
                .any(|item| item["label"] == *label && item["detail"] == "AIL source FS helper")),
        "completion must include AIL source FS helpers; got: {fs_items:?}"
    );

    let dotted_fs_completion_output = ail()
        .args(["lsp", "--complete", "fs.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_fs_completion = parse_json_output(&dotted_fs_completion_output);
    let dotted_fs_items = dotted_fs_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["fs.read_file", "fs.write", "fs.delete", "fs.list"]
            .iter()
            .all(|label| dotted_fs_items
                .iter()
                .any(|item| item["label"] == *label && item["detail"] == "AIL source FS helper")),
        "completion must include dotted FS helpers; got: {dotted_fs_items:?}"
    );

    let fs_hover_output = ail()
        .args(["lsp", "--hover-token", "fs_read_file", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let fs_hover = parse_json_output(&fs_hover_output);
    assert!(
        fs_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("path is Path or Text"),
        "hover must explain fs_read_file; got: {fs_hover}"
    );

    let numeric_completion_output = ail()
        .args(["lsp", "--complete", "numeric_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let numeric_completion = parse_json_output(&numeric_completion_output);
    let numeric_items = numeric_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        [
            "numeric_narrow_to_i32",
            "numeric_narrow_to_u32",
            "numeric_narrow_to_u64",
            "numeric_narrow_to_i16",
            "numeric_narrow_to_u8",
        ]
        .iter()
        .all(|label| numeric_items
            .iter()
            .any(|item| item["label"] == *label && item["detail"] == "AIL source Numeric helper")),
        "completion must include AIL source numeric narrow helpers; got: {numeric_items:?}"
    );

    let dotted_numeric_completion_output = ail()
        .args(["lsp", "--complete", "numeric.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_numeric_completion = parse_json_output(&dotted_numeric_completion_output);
    let dotted_numeric_items = dotted_numeric_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        [
            "numeric.narrow_to_i32",
            "numeric.narrow_to_u32",
            "numeric.narrow_to_u64",
            "numeric.narrow_to_i16",
            "numeric.narrow_to_u8",
        ]
        .iter()
        .all(|label| dotted_numeric_items
            .iter()
            .any(|item| item["label"] == *label && item["detail"] == "AIL source Numeric helper")),
        "completion must include dotted Numeric narrow helpers; got: {dotted_numeric_items:?}"
    );

    let numeric_hover_output = ail()
        .args(["lsp", "--hover-token", "numeric_narrow_to_u8", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let numeric_hover = parse_json_output(&numeric_hover_output);
    assert!(
        numeric_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("unsigned 8-bit range"),
        "hover must explain numeric_narrow_to_u8; got: {numeric_hover}"
    );

    let encoding_completion_output = ail()
        .args(["lsp", "--complete", "encoding_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let encoding_completion = parse_json_output(&encoding_completion_output);
    let encoding_items = encoding_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        [
            "encoding_base64_encode",
            "encoding_base64_decode",
            "encoding_hex_encode",
            "encoding_hex_decode",
        ]
        .iter()
        .all(|label| encoding_items
            .iter()
            .any(|item| item["label"] == *label && item["detail"] == "AIL source Encoding helper")),
        "completion must include AIL source encoding helpers; got: {encoding_items:?}"
    );

    let dotted_encoding_completion_output = ail()
        .args(["lsp", "--complete", "encoding.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_encoding_completion = parse_json_output(&dotted_encoding_completion_output);
    let dotted_encoding_items = dotted_encoding_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        [
            "encoding.base64_encode",
            "encoding.base64_decode",
            "encoding.hex_encode",
            "encoding.hex_decode",
        ]
        .iter()
        .all(|label| dotted_encoding_items
            .iter()
            .any(|item| item["label"] == *label && item["detail"] == "AIL source Encoding helper")),
        "completion must include dotted Encoding helpers; got: {dotted_encoding_items:?}"
    );

    let encoding_hover_output = ail()
        .args(["lsp", "--hover-token", "encoding_base64_decode", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let encoding_hover = parse_json_output(&encoding_hover_output);
    assert!(
        encoding_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("Result<Bytes,Text>"),
        "hover must explain encoding_base64_decode; got: {encoding_hover}"
    );

    let random_completion_output = ail()
        .args(["lsp", "--complete", "random_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let random_completion = parse_json_output(&random_completion_output);
    let random_items = random_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        random_items
            .iter()
            .any(|item| item["label"] == "random_next_int"
                && item["detail"] == "AIL source Random helper"),
        "completion must include random_next_int; got: {random_items:?}"
    );

    let time_completion_output = ail()
        .args(["lsp", "--complete", "time_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let time_completion = parse_json_output(&time_completion_output);
    let time_items = time_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        [
            "time_now",
            "time_duration_since",
            "time_add_duration",
            "time_instant_to_ms",
        ]
        .iter()
        .all(|label| time_items
            .iter()
            .any(|item| item["label"] == *label && item["detail"] == "AIL source Time helper")),
        "completion must include AIL source time helpers; got: {time_items:?}"
    );

    let dotted_time_completion_output = ail()
        .args(["lsp", "--complete", "time.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_time_completion = parse_json_output(&dotted_time_completion_output);
    let dotted_time_items = dotted_time_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        [
            "time.now",
            "time.duration_since",
            "time.add_duration",
            "time.instant_to_ms",
        ]
        .iter()
        .all(|label| dotted_time_items
            .iter()
            .any(|item| item["label"] == *label && item["detail"] == "AIL source Time helper")),
        "completion must include dotted Time helpers; got: {dotted_time_items:?}"
    );

    let time_hover_output = ail()
        .args(["lsp", "--hover-token", "time_duration_since", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let time_hover = parse_json_output(&time_hover_output);
    assert!(
        time_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("later_ms - earlier_ms"),
        "hover must explain time_duration_since; got: {time_hover}"
    );

    let bytes_completion_output = ail()
        .args(["lsp", "--complete", "bytes_", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let bytes_completion = parse_json_output(&bytes_completion_output);
    let bytes_items = bytes_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        [
            "bytes_length",
            "bytes_at",
            "bytes_slice",
            "bytes_concat",
            "bytes_empty",
        ]
        .iter()
        .all(|label| bytes_items
            .iter()
            .any(|item| item["label"] == *label && item["detail"] == "AIL source Bytes helper")),
        "completion must include AIL source bytes helpers; got: {bytes_items:?}"
    );

    let dotted_bytes_completion_output = ail()
        .args(["lsp", "--complete", "bytes.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_bytes_completion = parse_json_output(&dotted_bytes_completion_output);
    let dotted_bytes_items = dotted_bytes_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        [
            "bytes.length",
            "bytes.at",
            "bytes.slice",
            "bytes.concat",
            "bytes.empty",
        ]
        .iter()
        .all(|label| dotted_bytes_items
            .iter()
            .any(|item| item["label"] == *label && item["detail"] == "AIL source Bytes helper")),
        "completion must include dotted Bytes helpers; got: {dotted_bytes_items:?}"
    );

    let bytes_hover_output = ail()
        .args(["lsp", "--hover-token", "bytes_length", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let bytes_hover = parse_json_output(&bytes_hover_output);
    assert!(
        bytes_hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("byte count"),
        "hover must explain bytes_length; got: {bytes_hover}"
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

    let dotted_int_completion_output = ail()
        .args(["lsp", "--complete", "int.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_int_completion = parse_json_output(&dotted_int_completion_output);
    let dotted_int_items = dotted_int_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        [
            ("int.min", "AIL source Int bounds helper"),
            ("int.max", "AIL source Int bounds helper"),
            ("int.clamp", "AIL source Int bounds helper"),
            ("int.abs_or", "AIL source Int safety helper"),
            ("int.neg_or", "AIL source Int safety helper"),
            ("int.add_or", "AIL source Int safety helper"),
            ("int.sub_or", "AIL source Int safety helper"),
            ("int.mul_or", "AIL source Int safety helper"),
            ("int.div_or", "AIL source Int safety helper"),
            ("int.rem_or", "AIL source Int safety helper"),
            ("int.saturating_add", "AIL source Int safety helper"),
            ("int.saturating_sub", "AIL source Int safety helper"),
            ("int.saturating_mul", "AIL source Int safety helper"),
            ("int.saturating_neg", "AIL source Int safety helper"),
            (
                "int.wrapping_add",
                "AIL source Int explicit wrapping helper"
            ),
            (
                "int.wrapping_sub",
                "AIL source Int explicit wrapping helper"
            ),
            (
                "int.wrapping_mul",
                "AIL source Int explicit wrapping helper"
            ),
            (
                "int.wrapping_neg",
                "AIL source Int explicit wrapping helper"
            ),
            ("int.bit_and", "AIL source Int bitwise helper"),
            ("int.bit_or", "AIL source Int bitwise helper"),
            ("int.bit_xor", "AIL source Int bitwise helper"),
            ("int.bit_not", "AIL source Int bitwise helper"),
            ("int.shift_left", "AIL source Int bit shift helper"),
            ("int.shift_right", "AIL source Int bit shift helper"),
            (
                "int.shift_right_unsigned",
                "AIL source Int bit shift helper"
            ),
        ]
        .iter()
        .all(|(label, detail)| dotted_int_items
            .iter()
            .any(|item| item["label"] == *label && item["detail"] == *detail)),
        "completion must include dotted Int helpers; got: {dotted_int_items:?}"
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

    let dotted_text_completion_output = ail()
        .args(["lsp", "--complete", "text.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_text_completion = parse_json_output(&dotted_text_completion_output);
    let dotted_text_items = dotted_text_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        [
            ("text.length", "AIL source Text helper"),
            ("text.len", "AIL source Text helper"),
            ("text.is_empty", "AIL source Text predicate"),
            ("text.eq", "AIL source Text predicate"),
            ("text.trim", "AIL source Text helper"),
            ("text.contains", "AIL source Text predicate"),
            ("text.index_of", "AIL source Text search"),
            ("text.parse_int_or", "AIL source Text parser"),
            ("text.byte_at_or", "AIL source Text helper"),
            ("text.slice", "AIL source Text helper"),
            ("text.replace_first", "AIL source Text helper"),
            ("text.starts_with", "AIL source Text predicate"),
            ("text.ends_with", "AIL source Text predicate"),
        ]
        .iter()
        .all(|(label, detail)| dotted_text_items
            .iter()
            .any(|item| item["label"] == *label && item["detail"] == *detail)),
        "completion must include dotted Text helpers; got: {dotted_text_items:?}"
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

    let dotted_tuple_completion_output = ail()
        .args(["lsp", "--complete", "tuple.", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let dotted_tuple_completion = parse_json_output(&dotted_tuple_completion_output);
    let dotted_tuple_items = dotted_tuple_completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        ["tuple.length", "tuple.get", "tuple.first", "tuple.second"]
            .iter()
            .all(|label| {
                dotted_tuple_items.iter().any(|item| {
                    item["label"] == *label && item["detail"] == "AIL source Tuple helper"
                })
            }),
        "completion must include dotted Tuple helpers; got: {dotted_tuple_items:?}"
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
