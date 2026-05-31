use super::*;

#[test]
fn formats_source_builtin_calls_as_infix() {
    let (formatted, item_count) = format_ail_source(
        "test math = eq(add(sub(10, mul(2, 3)), add(div(8, 4), mod(7, 4))), 9)\n\
             test grouped = and(eq(sub(10, add(2, 3)), 5), not(false))\n\
             fn greeting(name: Text) -> Text = concat(\"Hello, \", name)\n",
    )
    .expect("source must format");

    assert_eq!(item_count, 3);
    assert!(formatted.contains("test math = 10 - 2 * 3 + (8 / 4 + 7 % 4) == 9\n"));
    assert!(formatted.contains("test grouped = 10 - (2 + 3) == 5 && !false\n"));
    assert!(formatted.contains("fn greeting(name: Text) -> Text = \"Hello, \" ++ name\n"));
}

#[test]
fn formats_source_int_bounds_helpers() {
    let (formatted, item_count) = format_ail_source(
            "fn low(left:Int,right:Int)->Int=int.min(left,right)\nfn high(left:Int,right:Int)->Int=int.max(left,right)\nfn bounded(value:Int,low:Int,high:Int)->Int=int.clamp(value,low,high)\nfn magnitude(value:Int,fallback:Int)->Int=int.abs_or(value,fallback)\nfn negated(value:Int,fallback:Int)->Int=int.neg_or(value,fallback)\nfn summed(left:Int,right:Int,fallback:Int)->Int=int.add_or(left,right,fallback)\nfn difference(left:Int,right:Int,fallback:Int)->Int=int.sub_or(left,right,fallback)\nfn product(left:Int,right:Int,fallback:Int)->Int=int.mul_or(left,right,fallback)\nfn saturated(left:Int,right:Int)->Int=int.saturating_add(left,right)\nfn saturated_difference(left:Int,right:Int)->Int=int.saturating_sub(left,right)\nfn saturated_product(left:Int,right:Int)->Int=int.saturating_mul(left,right)\nfn saturated_negated(value:Int)->Int=int.saturating_neg(value)\nfn wrapped_sum(left:Int,right:Int)->Int=int.wrapping_add(left,right)\nfn wrapped_difference(left:Int,right:Int)->Int=int.wrapping_sub(left,right)\nfn wrapped_product(left:Int,right:Int)->Int=int.wrapping_mul(left,right)\nfn wrapped_negated(value:Int)->Int=int.wrapping_neg(value)\nfn masked(left:Int,right:Int)->Int=int.bit_and(left,right)\nfn flagged(left:Int,right:Int)->Int=int.bit_or(left,right)\nfn toggled(left:Int,right:Int)->Int=int.bit_xor(left,right)\nfn inverted(value:Int)->Int=int.bit_not(value)\nfn shifted_left(value:Int,amount:Int)->Int=int.shift_left(value,amount)\nfn shifted_right(value:Int,amount:Int)->Int=int.shift_right(value,amount)\nfn shifted_right_unsigned(value:Int,amount:Int)->Int=int.shift_right_unsigned(value,amount)\nfn quotient(value:Int,divisor:Int,fallback:Int)->Int=int.div_or(value,divisor,fallback)\nfn remainder(value:Int,divisor:Int,fallback:Int)->Int=int.rem_or(value,divisor,fallback)\n",
        )
        .expect("source int bounds helpers must format");

    assert_eq!(item_count, 25);
    assert!(formatted.contains("fn low(left: Int, right: Int) -> Int = int_min(left, right)\n"));
    assert!(formatted.contains("fn high(left: Int, right: Int) -> Int = int_max(left, right)\n"));
    assert!(formatted.contains(
        "fn bounded(value: Int, low: Int, high: Int) -> Int = int_clamp(value, low, high)\n"
    ));
    assert!(formatted.contains(
        "fn magnitude(value: Int, fallback: Int) -> Int = int_abs_or(value, fallback)\n"
    ));
    assert!(
        formatted.contains(
            "fn negated(value: Int, fallback: Int) -> Int = int_neg_or(value, fallback)\n"
        )
    );
    assert!(formatted.contains(
            "fn summed(left: Int, right: Int, fallback: Int) -> Int = int_add_or(left, right, fallback)\n"
        ));
    assert!(formatted.contains(
            "fn difference(left: Int, right: Int, fallback: Int) -> Int = int_sub_or(left, right, fallback)\n"
        ));
    assert!(formatted.contains(
            "fn product(left: Int, right: Int, fallback: Int) -> Int = int_mul_or(left, right, fallback)\n"
        ));
    assert!(formatted.contains(
        "fn saturated(left: Int, right: Int) -> Int = int_saturating_add(left, right)\n"
    ));
    assert!(formatted.contains(
        "fn saturated_difference(left: Int, right: Int) -> Int = int_saturating_sub(left, right)\n"
    ));
    assert!(formatted.contains(
        "fn saturated_product(left: Int, right: Int) -> Int = int_saturating_mul(left, right)\n"
    ));
    assert!(
        formatted.contains("fn saturated_negated(value: Int) -> Int = int_saturating_neg(value)\n")
    );
    assert!(formatted.contains(
        "fn wrapped_sum(left: Int, right: Int) -> Int = int_wrapping_add(left, right)\n"
    ));
    assert!(formatted.contains(
        "fn wrapped_difference(left: Int, right: Int) -> Int = int_wrapping_sub(left, right)\n"
    ));
    assert!(formatted.contains(
        "fn wrapped_product(left: Int, right: Int) -> Int = int_wrapping_mul(left, right)\n"
    ));
    assert!(
        formatted.contains("fn wrapped_negated(value: Int) -> Int = int_wrapping_neg(value)\n")
    );
    assert!(
        formatted.contains("fn masked(left: Int, right: Int) -> Int = int_bit_and(left, right)\n")
    );
    assert!(
        formatted.contains("fn flagged(left: Int, right: Int) -> Int = int_bit_or(left, right)\n")
    );
    assert!(
        formatted.contains("fn toggled(left: Int, right: Int) -> Int = int_bit_xor(left, right)\n")
    );
    assert!(formatted.contains("fn inverted(value: Int) -> Int = int_bit_not(value)\n"));
    assert!(formatted.contains(
        "fn shifted_left(value: Int, amount: Int) -> Int = int_shift_left(value, amount)\n"
    ));
    assert!(formatted.contains(
        "fn shifted_right(value: Int, amount: Int) -> Int = int_shift_right(value, amount)\n"
    ));
    assert!(formatted.contains(
            "fn shifted_right_unsigned(value: Int, amount: Int) -> Int = int_shift_right_unsigned(value, amount)\n"
        ));
    assert!(formatted.contains(
            "fn quotient(value: Int, divisor: Int, fallback: Int) -> Int = int_div_or(value, divisor, fallback)\n"
        ));
    assert!(formatted.contains(
            "fn remainder(value: Int, divisor: Int, fallback: Int) -> Int = int_rem_or(value, divisor, fallback)\n"
        ));
}

#[test]
fn formats_source_pipe_operator_input_canonically() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn cleaned(value:Text)->Text=value |> text_trim() |> text_replace_first(" ", "_")
fn bounded(value:Int)->Int=value |> int_clamp(0,10)
"#,
    )
    .expect("source pipe operator input must format");

    assert_eq!(item_count, 2);
    assert!(formatted.contains(
        "fn cleaned(value: Text) -> Text = text_replace_first(text_trim(value), \" \", \"_\")\n"
    ));
    assert!(formatted.contains("fn bounded(value: Int) -> Int = int_clamp(value, 0, 10)\n"));
}

#[test]
fn formats_source_text_eq_helper() {
    let (formatted, item_count) =
        format_ail_source("fn same(left:Text,right:Text)->Bool=text.eq(left,right)\n")
            .expect("source text_eq must format");

    assert_eq!(item_count, 1);
    assert!(
        formatted.contains("fn same(left: Text, right: Text) -> Bool = text_eq(left, right)\n")
    );
}

#[test]
fn formats_source_text_trim_helper() {
    let (formatted, item_count) =
        format_ail_source("fn cleaned(value:Text)->Text=text.trim(value)\n")
            .expect("source text_trim must format");

    assert_eq!(item_count, 1);
    assert!(formatted.contains("fn cleaned(value: Text) -> Text = text_trim(value)\n"));
}

#[test]
fn formats_source_text_contains_helper() {
    let (formatted, item_count) = format_ail_source(
        "fn has(haystack:Text,needle:Text)->Bool=text.contains(haystack,needle)\n",
    )
    .expect("source text_contains must format");

    assert_eq!(item_count, 1);
    assert!(formatted.contains(
        "fn has(haystack: Text, needle: Text) -> Bool = text_contains(haystack, needle)\n"
    ));
}

#[test]
fn formats_source_text_index_of_helper() {
    let (formatted, item_count) = format_ail_source(
        "fn find(haystack:Text,needle:Text)->Int=text.index_of(haystack,needle)\n",
    )
    .expect("source text_index_of must format");

    assert_eq!(item_count, 1);
    assert!(formatted.contains(
        "fn find(haystack: Text, needle: Text) -> Int = text_index_of(haystack, needle)\n"
    ));
}

#[test]
fn formats_source_text_byte_at_or_helper() {
    let (formatted, item_count) = format_ail_source(
        "fn byte(value:Text,index:Int,fallback:Int)->Int=text.byte_at_or(value,index,fallback)\n",
    )
    .expect("source text_byte_at_or must format");

    assert_eq!(item_count, 1);
    assert!(formatted.contains(
            "fn byte(value: Text, index: Int, fallback: Int) -> Int = text_byte_at_or(value, index, fallback)\n"
        ));
}

#[test]
fn formats_source_text_parse_int_or_helper() {
    let (formatted, item_count) = format_ail_source(
        "fn parsed(value:Text,fallback:Int)->Int=text.parse_int_or(value,fallback)\n",
    )
    .expect("source text_parse_int_or must format");

    assert_eq!(item_count, 1);
    assert!(formatted.contains(
        "fn parsed(value: Text, fallback: Int) -> Int = text_parse_int_or(value, fallback)\n"
    ));
}

#[test]
fn formats_source_text_slice_helper() {
    let (formatted, item_count) = format_ail_source(
        "fn piece(value:Text,start:Int,length:Int)->Text=text.slice(value,start,length)\n",
    )
    .expect("source text_slice must format");

    assert_eq!(item_count, 1);
    assert!(formatted.contains(
            "fn piece(value: Text, start: Int, length: Int) -> Text = text_slice(value, start, length)\n"
        ));
}

#[test]
fn formats_source_text_replace_first_helper() {
    let (formatted, item_count) = format_ail_source(
            "fn changed(value:Text,needle:Text,replacement:Text)->Text=text.replace_first(value,needle,replacement)\n",
        )
        .expect("source text_replace_first must format");

    assert_eq!(item_count, 1);
    assert!(formatted.contains(
            "fn changed(value: Text, needle: Text, replacement: Text) -> Text = text_replace_first(value, needle, replacement)\n"
        ));
}

#[test]
fn formats_source_text_boundary_helpers() {
    let (formatted, item_count) = format_ail_source(
        "fn prefixed(haystack:Text,prefix:Text)->Bool=text.starts_with(haystack,prefix)\n\
             fn suffixed(haystack:Text,suffix:Text)->Bool=text.ends_with(haystack,suffix)\n",
    )
    .expect("source text boundary helpers must format");

    assert_eq!(item_count, 2);
    assert!(formatted.contains(
        "fn prefixed(haystack: Text, prefix: Text) -> Bool = text_starts_with(haystack, prefix)\n"
    ));
    assert!(formatted.contains(
        "fn suffixed(haystack: Text, suffix: Text) -> Bool = text_ends_with(haystack, suffix)\n"
    ));
}

#[test]
fn formats_source_unary_minus() {
    let (formatted, item_count) = format_ail_source(
        "fn negated(x:Int)->Int=sub(0,x)
             test grouped=eq(sub(0,add(1,2)),-3)
",
    )
    .expect("source must format");

    assert_eq!(item_count, 2);
    assert!(formatted.contains(
        "fn negated(x: Int) -> Int = -x
"
    ));
    assert!(formatted.contains(
        "test grouped = -(1 + 2) == -3
"
    ));
}

#[test]
fn formats_source_set_and_map_types() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn ids()->Set<i64>=set(1,add(2,3))
fn labels()->Map<String,int>=map("one",1,"two",2)
"#,
    )
    .expect("source set/map must format");

    assert_eq!(item_count, 2);
    assert!(formatted.contains("fn ids() -> Set<Int> = set(1, 2 + 3)\n"));
    assert!(formatted.contains("fn labels() -> Map<Text,Int> = map(\"one\", 1, \"two\", 2)\n"));
}

#[test]
fn formats_source_tuple_types() {
    let (formatted, item_count) =
        format_ail_source(r#"fn pair()->Tuple<i64,String>=tuple(42,"answer")"#)
            .expect("source tuple must format");

    assert_eq!(item_count, 1);
    assert!(formatted.contains("fn pair() -> Tuple<Int,Text> = tuple(42, \"answer\")\n"));
}

#[test]
fn formats_source_tuple_accessors() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn pair()->Tuple<i64,String>=tuple(42,"answer")
fn pair_len()->Int=tuple.length(pair())
fn pair_first()->Option<Int>=tuple.first(pair())
fn pair_second()->Option<Text>=tuple_second(pair())
fn pair_get()->Option<Text>=tuple.get(pair(),1)
"#,
    )
    .expect("source tuple accessors must format");

    assert_eq!(item_count, 5);
    assert!(formatted.contains("fn pair_len() -> Int = tuple_length(pair())\n"));
    assert!(formatted.contains("fn pair_first() -> Option<Int> = tuple_first(pair())\n"));
    assert!(formatted.contains("fn pair_second() -> Option<Text> = tuple_second(pair())\n"));
    assert!(formatted.contains("fn pair_get() -> Option<Text> = tuple_get(pair(), 1)\n"));
}

#[test]
fn formats_source_record_types() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn person()->Record<age:i64,name:String>=record(age,42,name,"Ada")
fn age()->Int=field(person(),age)
fn age_dot()->Int=person().age
fn older()->Record<age:Int,name:Text>=update(person(),age,43)
"#,
    )
    .expect("source record must format");

    assert_eq!(item_count, 4);
    assert!(
        formatted
            .contains("fn person() -> Record<age:Int,name:Text> = { age: 42, name: \"Ada\" }\n")
    );
    assert!(formatted.contains("fn age() -> Int = person().age\n"));
    assert!(formatted.contains("fn age_dot() -> Int = person().age\n"));
    assert!(
        formatted.contains("fn older() -> Record<age:Int,name:Text> = { ...person(), age: 43 }\n")
    );
}

#[test]
fn formats_source_option_result_constructors() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn maybe(flag:Bool)->Option<Int>=if flag { some(42) } else { none() }
fn ok_value()->Result<Int,Text>=ok(42)
fn err_value()->Result<Int,Text>=err("boom")
"#,
    )
    .expect("source constructors must format");

    assert_eq!(item_count, 3);
    assert!(
        formatted
            .contains("fn maybe(flag: Bool) -> Option<Int> = if flag { Some(42) } else { None }\n")
    );
    assert!(formatted.contains("fn ok_value() -> Result<Int,Text> = Ok(42)\n"));
    assert!(formatted.contains("fn err_value() -> Result<Int,Text> = Err(\"boom\")\n"));
}

#[test]
fn formats_source_unwrap_or_helper() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn value(input:Option<Int>)->Int=match(input,Some(v),v,None,0)
"#,
    )
    .expect("source unwrap_or must format");

    assert_eq!(item_count, 1);
    assert!(formatted.contains("fn value(input: Option<Int>) -> Int = unwrap_or(input, 0)\n"));
}

#[test]
fn formats_source_match_constructor_aliases_idempotently() {
    let src = "fn picked(input:Option<Int>,fallback:Int)->Int=match(input,some(v),if(gt(v,0),v,fallback),none(),fallback)\n";

    let (formatted, item_count) = format_ail_source(src).expect("source match must format");
    let (formatted_again, item_count_again) =
        format_ail_source(&formatted).expect("formatted source match must format again");

    assert_eq!(item_count, 1);
    assert_eq!(item_count_again, item_count);
    assert_eq!(formatted_again, formatted);
    assert_eq!(
        formatted,
        "fn picked(input: Option<Int>, fallback: Int) -> Int = match input { Some(v) => if v > 0 { v } else { fallback }, None => fallback }\n"
    );
}

#[test]
fn formats_source_option_predicate_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn has_value(input:Option<Int>)->Bool=match(input,Some(_),true,None,false)
fn missing(input:Option<Int>)->Bool=match(input,Some(_),false,None,true)
"#,
    )
    .expect("source option predicates must format");

    assert_eq!(item_count, 2);
    assert!(formatted.contains("fn has_value(input: Option<Int>) -> Bool = is_some(input)\n"));
    assert!(formatted.contains("fn missing(input: Option<Int>) -> Bool = is_none(input)\n"));
}

#[test]
fn formats_source_result_predicate_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn succeeded(input:Result<Int,Text>)->Bool=match(input,Ok(_),true,Err(_),false)
fn failed(input:Result<Int,Text>)->Bool=match(input,Ok(_),false,Err(_),true)
"#,
    )
    .expect("source result predicates must format");

    assert_eq!(item_count, 2);
    assert!(formatted.contains("fn succeeded(input: Result<Int,Text>) -> Bool = is_ok(input)\n"));
    assert!(formatted.contains("fn failed(input: Result<Int,Text>) -> Bool = is_err(input)\n"));
}

#[test]
fn formats_source_first_or_helper() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn first(values:List<Int>)->Int=if(gt(len(values),0),index(values,0),0)
"#,
    )
    .expect("source first_or must format");

    assert_eq!(item_count, 1);
    assert!(formatted.contains("fn first(values: List<Int>) -> Int = first_or(values, 0)\n"));
}

#[test]
fn formats_source_last_or_helper() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn last(values:List<Int>)->Int=if(gt(len(values),0),index(values,sub(len(values),1)),0)
"#,
    )
    .expect("source last_or must format");

    assert_eq!(item_count, 1);
    assert!(formatted.contains("fn last(values: List<Int>) -> Int = last_or(values, 0)\n"));
}

#[test]
fn formats_source_get_or_helper() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn item(values:List<Int>,idx:Int)->Int=if(and(ge(idx,0),lt(idx,len(values))),index(values,idx),0)
"#,
    )
    .expect("source get_or must format");

    assert_eq!(item_count, 1);
    assert!(
        formatted
            .contains("fn item(values: List<Int>, idx: Int) -> Int = get_or(values, idx, 0)\n")
    );
}

#[test]
fn formats_source_is_empty_helper() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn no_items(values:List<Int>)->Bool=eq(len(values),0)
fn no_text(value:Text)->Bool=eq(0,len(value))
"#,
    )
    .expect("source is_empty must format");

    assert_eq!(item_count, 2);
    assert!(formatted.contains("fn no_items(values: List<Int>) -> Bool = is_empty(values)\n"));
    assert!(formatted.contains("fn no_text(value: Text) -> Bool = is_empty(value)\n"));
}

#[test]
fn formats_source_project_fixture_idempotently() {
    let src = r#"
module app
use "./math.ail"
use "./types.ail"
capability log.write
const fallback:Int=0
fn person()->Record<name:Text,age:Int>=record(name,"Ada",age,42)
fn older()->Record<name:Text,age:Int>=update(person(),age,43)
fn chosen(status:Result<Int,Text>)->Int=match status { Ok(v) => int.bit_or(v, fallback()), Err(_) => fallback() }
fn main(input:Option<Int>)->Int{
let base:Int=unwrap_or(input,fallback())
let profile:Record<name:Text,age:Int>={ name: "Grace", age: base }
return if is_some(input) { add(profile.age, chosen(Ok(1))) } else { fallback() }
}
test main_missing=eq(main(None),0)
grant main log.write
"#;

    let (formatted, item_count) = format_ail_source(src).expect("project fixture must format");
    let (formatted_again, item_count_again) =
        format_ail_source(&formatted).expect("formatted source must format again");

    assert_eq!(item_count, 11);
    assert_eq!(item_count_again, item_count);
    assert_eq!(formatted_again, formatted);
    assert_eq!(
        formatted,
        "module app\n\
use \"./math.ail\"\n\
use \"./types.ail\"\n\
capability log.write\n\
const fallback: Int = 0\n\
fn person() -> Record<name:Text,age:Int> = { age: 42, name: \"Ada\" }\n\
fn older() -> Record<name:Text,age:Int> = { ...person(), age: 43 }\n\
fn chosen(status: Result<Int,Text>) -> Int = match status { Ok(v) => int_bit_or(v, fallback), Err(_) => fallback }\n\
fn main(input: Option<Int>) -> Int {\n\
  let base: Int = unwrap_or(input, fallback)\n\
  let profile: Record<name:Text,age:Int> = { age: base, name: \"Grace\" }\n\
  return if is_some(input) { profile.age + chosen(Ok(1)) } else { fallback }\n\
}\n\
test main_missing = main(None) == 0\n\
grant main log.write\n"
    );
}

#[test]
fn formats_source_strings_without_treating_slashes_as_comments() {
    let src = r#"
fn message()->Text=concat("https://ail.local", " {ok}") // trailing comment
"#;
    let (formatted, item_count) = format_ail_source(src).expect("source must format");

    assert_eq!(item_count, 1);
    assert_eq!(
        formatted,
        "fn message() -> Text = \"https://ail.local\" ++ \" {ok}\"\n"
    );
}

#[test]
fn formats_source_capabilities_and_grants() {
    let src = r#"
grant fn.print_hello log.write
fn print_hello()->Int=print("Hello")
capability log.write
"#;
    let (formatted, item_count) = format_ail_source(src).expect("source must format");

    assert_eq!(item_count, 3);
    assert_eq!(
        formatted,
        "capability log.write\nfn print_hello() -> Int = print(\"Hello\")\ngrant print_hello log.write\n"
    );
}

#[test]
fn formats_ail_source_with_params_blocks_and_if() {
    let src = r#"
fn add_pair(x:Int,y:Int)->Int=add(x,y)
fn main()->Int{
let base=add(20,20)
return if gt(base,40){add(base,2)} else {0}
}
test addition=eq(add_pair(20,22),42)
"#;
    let (formatted, item_count) = format_ail_source(src).expect("source must format");

    assert_eq!(item_count, 3);
    assert!(formatted.contains("fn add_pair(x: Int, y: Int) -> Int = x + y\n"));
    assert!(formatted.contains("fn main() -> Int {\n"));
    assert!(formatted.contains("  let base = 20 + 20\n"));
    assert!(formatted.contains("  return if base > 40 { base + 2 } else { 0 }\n"));
    assert!(formatted.contains("test addition = add_pair(20, 22) == 42\n"));
}
