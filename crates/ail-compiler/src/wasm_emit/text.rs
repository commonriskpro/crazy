use super::control::emit_local_as_i64;
use super::*;

mod boundary;
mod core;
mod parse;
mod replace;
mod search;
mod slice;
mod trim;

pub(super) use self::boundary::emit_text_boundary_match;
pub(super) use self::core::{
    emit_list_len_from_local, emit_text_concat, emit_text_len_from_local, emit_text_ptr_from_local,
    load_i32_u8_at,
};
pub(super) use self::parse::emit_text_parse_int_or;
pub(super) use self::replace::emit_text_replace_first;
pub(super) use self::search::{emit_text_contains, emit_text_eq, emit_text_index_of};
pub(super) use self::slice::{emit_text_byte_at_or, emit_text_slice};
pub(super) use self::trim::{emit_ascii_whitespace_test_from_local, emit_text_trim};
