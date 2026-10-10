//! Row tokenizing used by the client's map and fish readers.

/// Removes the characters the client's `Trim` strips: every character up to
/// and including the space.
pub(super) fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| c <= ' ')
}

/// Splits off the leading token of a row the way the client does: the row is
/// trimmed and then cut at its first tab, or at its first space when it has no
/// tab. The remainder keeps any whitespace that follows the delimiter.
pub(super) fn split_token(text: &str) -> (&str, &str) {
    let text = trim(text);
    match text.find('\t').or_else(|| text.find(' ')) {
        Some(at) => (&text[..at], &text[at + 1..]),
        None => (text, ""),
    }
}

/// Splits a whole row into tokens with repeated [`split_token`] calls.
pub(super) fn tokens(mut text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    loop {
        let (token, rest) = split_token(text);
        if token.is_empty() {
            return tokens;
        }
        tokens.push(token);
        text = rest;
    }
}

/// Parses a token like the client's `StrToIntDef`: leading spaces, then a
/// signed decimal number or a hexadecimal number prefixed with `$`, `x` or
/// `0x`. `None` marks text for which the client falls back to its default.
pub(super) fn client_int(token: &str) -> Option<i32> {
    let text = token.trim_start_matches(' ');
    let hex = text
        .strip_prefix(['$', 'x', 'X'])
        .or_else(|| text.strip_prefix("0x"))
        .or_else(|| text.strip_prefix("0X"));
    match hex {
        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit()) => {
            u32::from_str_radix(digits, 16)
                .ok()
                .map(|value| value as i32)
        }
        Some(_) => None,
        None => text.parse().ok(),
    }
}

/// Reads `N` leading values like the client, which stores -1 for a missing or
/// non-numeric token. Returns the values, whether any of them fell back to -1,
/// and the untouched remainder of the row.
pub(super) fn leading_values<const N: usize>(mut text: &str) -> ([i32; N], bool, &str) {
    let mut values = [-1; N];
    let mut normalized = false;
    for value in &mut values {
        let (token, rest) = split_token(text);
        text = rest;
        match client_int(token) {
            Some(parsed) => *value = parsed,
            None => normalized = true,
        }
    }
    (values, normalized, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_split_at_the_first_tab_before_any_space() {
        assert_eq!(split_token("  DATA 1\t2 3 "), ("DATA 1", "2 3"));
        assert_eq!(split_token("D 2 30"), ("D", "2 30"));
        assert_eq!(split_token("S\t 2"), ("S", " 2"));
        assert_eq!(split_token("VNUM"), ("VNUM", ""));
        assert_eq!(split_token(" \t\r"), ("", ""));
        assert_eq!(tokens("1\t\t10 \t5"), ["1", "10 ", "5"]);
        assert_eq!(tokens("1  2 3"), ["1", "2", "3"]);
    }

    #[test]
    fn numbers_follow_the_client_conversion() {
        assert_eq!(client_int("42"), Some(42));
        assert_eq!(client_int("+7"), Some(7));
        assert_eq!(client_int("  -3"), Some(-3));
        assert_eq!(client_int("$1f"), Some(31));
        assert_eq!(client_int("0x10"), Some(16));
        assert_eq!(client_int("$FFFFFFFF"), Some(-1));
        assert_eq!(client_int("-2147483648"), Some(i32::MIN));
        for token in [
            "",
            "-",
            "$",
            "0x",
            "1 ",
            "\t2",
            "2 // c",
            "abc",
            "$100000000",
            "2147483648",
        ] {
            assert_eq!(client_int(token), None, "{token:?}");
        }
    }

    #[test]
    fn leading_values_keep_the_raw_remainder() {
        assert_eq!(
            leading_values("20 30 6 2  Two Words "),
            ([20, 30, 6, 2], false, " Two Words")
        );
        assert_eq!(leading_values("40\t50"), ([40, 50, -1, -1], true, ""));
        assert_eq!(leading_values("1 10\t5 1 name"), ([-1, 5, 1, -1], true, ""));
    }
}
