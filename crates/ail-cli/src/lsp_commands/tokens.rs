use super::source_helpers::is_acl_token_char;

pub(super) fn token_at_position(text: &str, line: usize, character: usize) -> Option<String> {
    let line_text = text.lines().nth(line)?;
    let char_indices: Vec<(usize, char)> = line_text.char_indices().collect();
    let byte_pos = char_indices
        .get(character)
        .map(|(idx, _)| *idx)
        .unwrap_or(line_text.len());
    if let Some(operator) = source_operator_token_at_position(line_text, byte_pos) {
        return Some(operator.to_string());
    }
    let start = line_text[..byte_pos]
        .char_indices()
        .rev()
        .find(|(_, ch)| !is_acl_token_char(*ch))
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);
    let end = line_text[byte_pos..]
        .char_indices()
        .find(|(_, ch)| !is_acl_token_char(*ch))
        .map(|(idx, _)| byte_pos + idx)
        .unwrap_or(line_text.len());
    (start < end).then(|| line_text[start..end].to_string())
}

fn source_operator_token_at_position(line: &str, byte_pos: usize) -> Option<&'static str> {
    const OPERATORS: &[&str] = &[
        "&&", "||", "==", "!=", ">=", "<=", "+", "-", "*", "/", "%", "!", ">", "<",
    ];

    OPERATORS.iter().copied().find(|operator| {
        line.match_indices(operator).any(|(start, _)| {
            let end = start + operator.len();
            byte_pos >= start && byte_pos <= end
        })
    })
}
