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
fn formats_source_crypto_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn digest(value:Bytes)->Bytes=std.crypto.hash(value)
fn mac(key:Bytes,message:Bytes)->Bytes=crypto.hmac(key,message)
fn same(left:Bytes,right:Bytes)->Bool=std.crypto.constant_time_eq(left,right)
"#,
    )
    .expect("source crypto helpers must format");

    assert_eq!(item_count, 3);
    assert!(formatted.contains("fn digest(value: Bytes) -> Bytes = crypto_hash(value)\n"));
    assert!(
        formatted
            .contains("fn mac(key: Bytes, message: Bytes) -> Bytes = crypto_hmac(key, message)\n")
    );
    assert!(formatted.contains(
        "fn same(left: Bytes, right: Bytes) -> Bool = crypto_constant_time_eq(left, right)\n"
    ));
}

#[test]
fn formats_source_numeric_narrow_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn i32ish(value:Int)->Result<Int,Text>=std.numeric.narrow_to_i32(value)
fn u32ish(value:Int)->Result<Int,Text>=numeric.narrow_to_u32(value)
fn u64ish(value:Int)->Result<Int,Text>=std.numeric.narrow_to_u64(value)
fn i16ish(value:Int)->Result<Int,Text>=numeric.narrow_to_i16(value)
fn byteish(value:Int)->Result<Int,Text>=std.numeric.narrow_to_u8(value)
"#,
    )
    .expect("source numeric narrow helpers must format");

    assert_eq!(item_count, 5);
    assert!(
        formatted
            .contains("fn i32ish(value: Int) -> Result<Int,Text> = numeric_narrow_to_i32(value)\n")
    );
    assert!(
        formatted
            .contains("fn u32ish(value: Int) -> Result<Int,Text> = numeric_narrow_to_u32(value)\n")
    );
    assert!(
        formatted
            .contains("fn u64ish(value: Int) -> Result<Int,Text> = numeric_narrow_to_u64(value)\n")
    );
    assert!(
        formatted
            .contains("fn i16ish(value: Int) -> Result<Int,Text> = numeric_narrow_to_i16(value)\n")
    );
    assert!(
        formatted
            .contains("fn byteish(value: Int) -> Result<Int,Text> = numeric_narrow_to_u8(value)\n")
    );
}

#[test]
fn formats_source_json_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn parsed(value:Text)->Result<Json,Text>=std.json.parse(value)
fn emitted(value:Json)->Text=json.stringify(value)
"#,
    )
    .expect("source json helpers must format");

    assert_eq!(item_count, 2);
    assert!(
        formatted.contains("fn parsed(value: Text) -> Result<Json,Text> = json_parse(value)\n")
    );
    assert!(formatted.contains("fn emitted(value: Json) -> Text = json_stringify(value)\n"));
}

#[test]
fn formats_source_encoding_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn b64(value:Bytes)->Text=std.encoding.base64_encode(value)
fn from_b64(value:Text)->Result<Bytes,Text>=encoding.base64_decode(value)
fn hexed(value:Bytes)->Text=std.encoding.hex_encode(value)
fn from_hex(value:Text)->Result<Bytes,Text>=encoding.hex_decode(value)
"#,
    )
    .expect("source encoding helpers must format");

    assert_eq!(item_count, 4);
    assert!(formatted.contains("fn b64(value: Bytes) -> Text = encoding_base64_encode(value)\n"));
    assert!(formatted.contains(
        "fn from_b64(value: Text) -> Result<Bytes,Text> = encoding_base64_decode(value)\n"
    ));
    assert!(formatted.contains("fn hexed(value: Bytes) -> Text = encoding_hex_encode(value)\n"));
    assert!(
        formatted.contains(
            "fn from_hex(value: Text) -> Result<Bytes,Text> = encoding_hex_decode(value)\n"
        )
    );
}

#[test]
fn formats_source_time_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn elapsed(later:Int,earlier:Int)->Int=std.time.duration_since(later,earlier)
fn deadline(start:Int,delta:Int)->Int=time.add_duration(start,delta)
fn millis(value:Int)->Int=std.time.instant_to_ms(value)
"#,
    )
    .expect("source time helpers must format");

    assert_eq!(item_count, 3);
    assert!(formatted.contains(
        "fn elapsed(later: Int, earlier: Int) -> Int = time_duration_since(later, earlier)\n"
    ));
    assert!(formatted.contains(
        "fn deadline(start: Int, delta: Int) -> Int = time_add_duration(start, delta)\n"
    ));
    assert!(formatted.contains("fn millis(value: Int) -> Int = time_instant_to_ms(value)\n"));
}

#[test]
fn formats_source_bytes_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn count(input:bytes)->Int=std.bytes.length(input)
fn maybe_byte(input:Bytes)->Option<Int>=bytes.at(input,0)
fn piece(input:Bytes)->Option<Bytes>=std.bytes.slice(input,0,2)
fn merged(left:Bytes,right:Bytes)->Bytes=bytes.concat(left,right)
fn empty(input:Bytes)->Bool=std.bytes.empty(input)
"#,
    )
    .expect("source bytes helpers must format");

    assert_eq!(item_count, 5);
    assert!(formatted.contains("fn count(input: Bytes) -> Int = bytes_length(input)\n"));
    assert!(
        formatted.contains("fn maybe_byte(input: Bytes) -> Option<Int> = bytes_at(input, 0)\n")
    );
    assert!(
        formatted.contains("fn piece(input: Bytes) -> Option<Bytes> = bytes_slice(input, 0, 2)\n")
    );
    assert!(
        formatted.contains(
            "fn merged(left: Bytes, right: Bytes) -> Bytes = bytes_concat(left, right)\n"
        )
    );
    assert!(formatted.contains("fn empty(input: Bytes) -> Bool = bytes_empty(input)\n"));
}

#[test]
fn formats_source_decimal_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn amount(cents:Int)->Tuple<Int,Int>=std.decimal.from_int(cents)
fn scaled(value:Tuple<Int,Int>)->Result<Tuple<Int,Int>,Text>=decimal.rescale(value,2)
fn summed(left:Tuple<Int,Int>,right:Tuple<Int,Int>)->Result<Tuple<Int,Int>,Text>=std.decimal.add(left,right)
fn difference(left:Tuple<Int,Int>,right:Tuple<Int,Int>)->Result<Tuple<Int,Int>,Text>=decimal.sub(left,right)
fn product(left:Tuple<Int,Int>,right:Tuple<Int,Int>)->Result<Tuple<Int,Int>,Text>=std.decimal.mul(left,right)
fn negative(value:Tuple<Int,Int>)->Bool=decimal.is_negative(value)
fn zero(value:Tuple<Int,Int>)->Bool=std.decimal.is_zero(value)
fn safe(value:Tuple<Int,Int>)->Result<Tuple<Int,Int>,Text>=decimal.non_negative(value)
"#,
    )
    .expect("source decimal helpers must format");

    assert_eq!(item_count, 8);
    assert!(
        formatted.contains("fn amount(cents: Int) -> Tuple<Int,Int> = decimal_from_int(cents)\n")
    );
    assert!(formatted.contains(
        "fn scaled(value: Tuple<Int,Int>) -> Result<Tuple<Int,Int>,Text> = decimal_rescale(value, 2)\n"
    ));
    assert!(formatted.contains(
        "fn summed(left: Tuple<Int,Int>, right: Tuple<Int,Int>) -> Result<Tuple<Int,Int>,Text> = decimal_add(left, right)\n"
    ));
    assert!(formatted.contains(
        "fn difference(left: Tuple<Int,Int>, right: Tuple<Int,Int>) -> Result<Tuple<Int,Int>,Text> = decimal_sub(left, right)\n"
    ));
    assert!(formatted.contains(
        "fn product(left: Tuple<Int,Int>, right: Tuple<Int,Int>) -> Result<Tuple<Int,Int>,Text> = decimal_mul(left, right)\n"
    ));
    assert!(
        formatted
            .contains("fn negative(value: Tuple<Int,Int>) -> Bool = decimal_is_negative(value)\n")
    );
    assert!(
        formatted.contains("fn zero(value: Tuple<Int,Int>) -> Bool = decimal_is_zero(value)\n")
    );
    assert!(formatted.contains(
        "fn safe(value: Tuple<Int,Int>) -> Result<Tuple<Int,Int>,Text> = decimal_non_negative(value)\n"
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
fn formats_source_list_mutation_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn values()->List<Int>=list(1,2)
fn pushed()->List<Int>=list.push(values(),3)
fn merged()->List<Int>=list.concat(values(),list(3,4))
"#,
    )
    .expect("source list helpers must format");

    assert_eq!(item_count, 3);
    assert!(formatted.contains(
        "fn pushed() -> List<Int> = list_push(values(), 3)
"
    ));
    assert!(formatted.contains(
        "fn merged() -> List<Int> = list_concat(values(), [3, 4])
"
    ));
}

#[test]
fn formats_source_queue_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn queue()->List<Int>=list(1,2)
fn pushed()->List<Int>=queue.push_back(queue(),3)
fn popped()->Option<Tuple<Int,List<Int>>>=queue.pop_front(queue())
fn peeked()->Option<Int>=queue.peek_front(queue())
fn count()->Int=queue.length(queue())
fn empty()->Bool=queue.is_empty(queue())
"#,
    )
    .expect("source queue helpers must format");

    assert_eq!(item_count, 6);
    assert!(formatted.contains(
        "fn pushed() -> List<Int> = queue_push_back(queue(), 3)
"
    ));
    assert!(formatted.contains(
        "fn popped() -> Option<Tuple<Int,List<Int>>> = queue_pop_front(queue())
"
    ));
    assert!(formatted.contains(
        "fn peeked() -> Option<Int> = queue_peek_front(queue())
"
    ));
    assert!(formatted.contains(
        "fn count() -> Int = queue_length(queue())
"
    ));
    assert!(formatted.contains(
        "fn empty() -> Bool = queue_is_empty(queue())
"
    ));
}

#[test]
fn formats_source_set_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn ids()->Set<Int>=set(1,2)
fn has_two()->Bool=set.contains(ids(),2)
fn count()->Int=set.length(ids())
fn updated()->Set<Int>=set.insert(ids(),3)
"#,
    )
    .expect("source set helpers must format");

    assert_eq!(item_count, 4);
    assert!(formatted.contains(
        "fn has_two() -> Bool = set_contains(ids(), 2)
"
    ));
    assert!(formatted.contains(
        "fn count() -> Int = set_length(ids())
"
    ));
    assert!(formatted.contains(
        "fn updated() -> Set<Int> = set_insert(ids(), 3)
"
    ));
}

#[test]
fn formats_source_map_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn labels()->Map<Text,Int>=map("one",1)
fn maybe()->Option<Int>=map.get(labels(),"one")
fn has()->Bool=map.contains_key(labels(),"one")
fn count()->Int=map.length(labels())
fn updated()->Map<Text,Int>=map.insert(labels(),"two",2)
"#,
    )
    .expect("source map helpers must format");

    assert_eq!(item_count, 5);
    assert!(formatted.contains(
        r#"fn maybe() -> Option<Int> = map_get(labels(), "one")
"#
    ));
    assert!(formatted.contains(
        r#"fn has() -> Bool = map_contains_key(labels(), "one")
"#
    ));
    assert!(formatted.contains("fn count() -> Int = map_length(labels())\n"));
    assert!(formatted.contains(
        r#"fn updated() -> Map<Text,Int> = map_insert(labels(), "two", 2)
"#
    ));
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
fn formats_source_option_result_conversion_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn value(input:Result<Int,Text>)->Int=match(input,Ok(v),v,Err(_),0)
fn promoted(input:Option<Int>)->Result<Int,Text>=match(input,Some(v),Ok(v),None,Err("missing"))
fn dotted(input:Result<Int,Text>, maybe:Option<Int>)->Int=add(result.unwrap_or(input,0),option.unwrap_or(maybe,1))
"#,
    )
    .expect("source option/result conversion helpers must format");

    assert_eq!(item_count, 3);
    assert!(
        formatted
            .contains("fn value(input: Result<Int,Text>) -> Int = result_unwrap_or(input, 0)\n")
    );
    assert!(formatted.contains(
        r#"fn promoted(input: Option<Int>) -> Result<Int,Text> = ok_or(input, "missing")
"#
    ));
    assert!(formatted.contains(
        "fn dotted(input: Result<Int,Text>, maybe: Option<Int>) -> Int = result_unwrap_or(input, 0) + option_unwrap_or(maybe, 1)\n"
    ));
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
fn namespaced(input:Option<Int>)->Bool=option.is_some(input)
"#,
    )
    .expect("source option predicates must format");

    assert_eq!(item_count, 3);
    assert!(formatted.contains("fn has_value(input: Option<Int>) -> Bool = is_some(input)\n"));
    assert!(formatted.contains("fn missing(input: Option<Int>) -> Bool = is_none(input)\n"));
    assert!(
        formatted.contains("fn namespaced(input: Option<Int>) -> Bool = option_is_some(input)\n")
    );
}

#[test]
fn formats_source_result_predicate_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn succeeded(input:Result<Int,Text>)->Bool=match(input,Ok(_),true,Err(_),false)
fn failed(input:Result<Int,Text>)->Bool=match(input,Ok(_),false,Err(_),true)
fn namespaced(input:Result<Int,Text>)->Bool=result.is_err(input)
"#,
    )
    .expect("source result predicates must format");

    assert_eq!(item_count, 3);
    assert!(formatted.contains("fn succeeded(input: Result<Int,Text>) -> Bool = is_ok(input)\n"));
    assert!(formatted.contains("fn failed(input: Result<Int,Text>) -> Bool = is_err(input)\n"));
    assert!(
        formatted
            .contains("fn namespaced(input: Result<Int,Text>) -> Bool = result_is_err(input)\n")
    );
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
fn formats_source_list_get_helper() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn maybe_item(values:List<Int>,idx:Int)->Option<Int>=if(and(ge(idx,0),lt(idx,len(values))),some(index(values,idx)),none())
fn dotted(values:List<Int>,idx:Int)->Option<Int>=list.get(values,idx)
"#,
    )
    .expect("source list_get must format");

    assert_eq!(item_count, 2);
    assert!(formatted.contains(
        "fn maybe_item(values: List<Int>, idx: Int) -> Option<Int> = list_get(values, idx)\n"
    ));
    assert!(formatted.contains(
        "fn dotted(values: List<Int>, idx: Int) -> Option<Int> = list_get(values, idx)\n"
    ));
}

#[test]
fn formats_source_is_empty_helper() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn no_items(values:List<Int>)->Bool=eq(len(values),0)
fn no_text(value:Text)->Bool=eq(0,len(value))
fn no_list_named(values:List<Int>)->Bool=list.is_empty(values)
fn no_text_named(value:Text)->Bool=text.is_empty(value)
"#,
    )
    .expect("source is_empty must format");

    assert_eq!(item_count, 4);
    assert!(formatted.contains("fn no_items(values: List<Int>) -> Bool = is_empty(values)\n"));
    assert!(formatted.contains("fn no_text(value: Text) -> Bool = is_empty(value)\n"));
    assert!(
        formatted.contains("fn no_list_named(values: List<Int>) -> Bool = list_is_empty(values)\n")
    );
    assert!(formatted.contains("fn no_text_named(value: Text) -> Bool = text_is_empty(value)\n"));
}

#[test]
fn formats_source_length_alias_helpers() {
    let (formatted, item_count) = format_ail_source(
        r#"
fn text_len(value:Text)->Int=text.length(value)
fn list_len(values:List<Int>)->Int=list.length(values)
"#,
    )
    .expect("source length aliases must format");

    assert_eq!(item_count, 2);
    assert!(formatted.contains("fn text_len(value: Text) -> Int = text_length(value)\n"));
    assert!(formatted.contains("fn list_len(values: List<Int>) -> Int = list_length(values)\n"));
}

#[test]
fn formats_source_block_expression_statements() {
    let (formatted, item_count) = format_ail_source(
        r#"capability log.write
fn main()->Unit{
log.write("hi")
return ()
}
grant main log.write
"#,
    )
    .expect("source block expression statement must format");

    assert_eq!(item_count, 3);
    assert_eq!(
        formatted,
        "capability log.write\n\
fn main() -> Unit {\n\
  print(\"hi\")\n\
  return ()\n\
}\n\
grant main log.write\n"
    );
}

#[test]
fn formats_source_control_block_expression_statements() {
    let (formatted, item_count) = format_ail_source(
        r#"capability log.write
fn main(flag:Bool)->Int{
if flag {
log.write("then")
return 1
} else {
log.write("else")
return 0
}
}
grant main log.write
"#,
    )
    .expect("source control block expression statement must format");

    assert_eq!(item_count, 3);
    assert!(
        formatted.contains(
            "return if flag {\n\
  print(\"then\")\n\
  return 1\n\
} else {\n\
  print(\"else\")\n\
  return 0\n\
}\n"
        ),
        "formatter must preserve expression statements in if branches; got:\n{formatted}"
    );
}

#[test]
fn formats_source_match_arm_expression_statements() {
    let (formatted, item_count) = format_ail_source(
        r#"capability log.write
fn main(value:Option<Int>)->Int{
match value {
Some(v) => {
log.write("some")
return v
}
None => {
log.write("none")
return 0
}
}
}
grant main log.write
"#,
    )
    .expect("source match arm expression statement must format");

    assert_eq!(item_count, 3);
    assert!(
        formatted.contains(
            "match value {\n\
  Some(v) => {\n\
    print(\"some\")\n\
    return v\n\
  }\n\
  None => {\n\
    print(\"none\")\n\
    return 0\n\
  }\n\
}\n"
        ),
        "formatter must preserve expression statements in match arms; got:\n{formatted}"
    );
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
fn log_hello()->Int=log.write("Hello")
capability log.write
grant log_hello log.write
"#;
    let (formatted, item_count) = format_ail_source(src).expect("source must format");

    assert_eq!(item_count, 5);
    assert_eq!(
        formatted,
        "capability log.write\nfn print_hello() -> Int = print(\"Hello\")\nfn log_hello() -> Int = log_write(\"Hello\")\ngrant print_hello log.write\ngrant log_hello log.write\n"
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
