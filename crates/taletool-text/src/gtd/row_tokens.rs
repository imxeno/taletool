//! Row tokenizing shared by the NSgtdData readers.
//!
//! Most client readers trim each row, split it into tokens at tabs or spaces
//! with [`split_token`], and convert integer tokens with [`client_int`],
//! storing a per-field default when a token is missing or not a number.

/// Whether the client strips `c` when it trims rows and text fields: spaces
/// and control characters.
pub(super) fn is_trimmed(c: char) -> bool {
    c <= ' '
}

/// Trims the characters the client strips from both ends of rows and text
/// fields.
pub(super) fn trim(text: &str) -> &str {
    text.trim_matches(is_trimmed)
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
pub(super) fn tokens(mut text: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        let (token, rest) = split_token(text);
        text = rest;
        (!token.is_empty()).then_some(token)
    })
}

/// Parses a token the way the client converts integers: leading spaces, an
/// optional sign, then a decimal number or a hexadecimal number prefixed with
/// `$`, `x`/`X`, or `0x`/`0X`. Decimal numbers must fit in an `i32`, while
/// hexadecimal numbers may use all 32 bits. The client stops reading at a NUL
/// character. `None` marks text for which the client stores the field's
/// default instead.
pub(super) fn client_int(token: &str) -> Option<i32> {
    let text = token.split('\0').next().unwrap_or_default();
    let text = text.trim_start_matches(' ');
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let hex = unsigned
        .strip_prefix(['$', 'x', 'X'])
        .or_else(|| unsigned.strip_prefix("0x"))
        .or_else(|| unsigned.strip_prefix("0X"));
    match hex {
        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit()) => {
            let value = u32::from_str_radix(digits, 16).ok()? as i32;
            Some(if text.starts_with('-') {
                value.wrapping_neg()
            } else {
                value
            })
        }
        Some(_) => None,
        None => text.parse().ok(),
    }
}

/// Reads the `N` leading tokens of `text` as integers, storing the matching
/// entry of `defaults` for a missing or non-numeric token like the client.
/// Returns the values, whether any of them took its default, and the untouched
/// remainder after the tokens read.
pub(super) fn leading_values<const N: usize>(
    mut text: &str,
    defaults: [i32; N],
) -> ([i32; N], bool, &str) {
    let mut defaulted = false;
    let values = defaults.map(|default| {
        let (token, rest) = split_token(text);
        text = rest;
        client_int(token).unwrap_or_else(|| {
            defaulted = true;
            default
        })
    });
    (values, defaulted, text)
}

/// Reads every token of `text` as an integer, storing `default` for a
/// non-numeric token. Returns the values and whether any of them took the
/// default.
pub(super) fn all_values(text: &str, default: i32) -> (Vec<i32>, bool) {
    let mut defaulted = false;
    let values = tokens(text)
        .map(|token| {
            client_int(token).unwrap_or_else(|| {
                defaulted = true;
                default
            })
        })
        .collect();
    (values, defaulted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimming_strips_spaces_and_control_characters_only() {
        assert_eq!(trim(" \t\x01a b\x1f\r"), "a b");
        assert_eq!(trim("\u{a0}a\u{3000}"), "\u{a0}a\u{3000}");
    }

    #[test]
    fn rows_split_at_the_first_tab_before_any_space() {
        assert_eq!(split_token("  DATA 1\t2 3 "), ("DATA 1", "2 3"));
        assert_eq!(split_token("D 2 30"), ("D", "2 30"));
        assert_eq!(split_token("S\t 2"), ("S", " 2"));
        assert_eq!(split_token("VNUM"), ("VNUM", ""));
        assert_eq!(split_token(" \t\r"), ("", ""));
        assert_eq!(tokens("1\t\t10 \t5").collect::<Vec<_>>(), ["1", "10 ", "5"]);
        assert_eq!(tokens("1  2 3").collect::<Vec<_>>(), ["1", "2", "3"]);
    }

    #[test]
    fn numbers_follow_the_client_conversion() {
        assert_eq!(client_int("42"), Some(42));
        assert_eq!(client_int("+7"), Some(7));
        assert_eq!(client_int("  -3"), Some(-3));
        assert_eq!(client_int("$1f"), Some(31));
        assert_eq!(client_int("0x10"), Some(16));
        assert_eq!(client_int("X10"), Some(16));
        assert_eq!(client_int("0XfF"), Some(255));
        assert_eq!(client_int("$FFFFFFFF"), Some(-1));
        assert_eq!(client_int("-2147483648"), Some(i32::MIN));
        assert_eq!(client_int("-$1F"), Some(-31));
        assert_eq!(client_int("+0x10"), Some(16));
        assert_eq!(client_int(" -x2"), Some(-2));
        assert_eq!(client_int("-$FFFFFFFF"), Some(1));
        assert_eq!(client_int("7\0junk"), Some(7));
        assert_eq!(client_int("$1f\0 "), Some(31));
        for token in [
            "",
            "\0",
            "-",
            "-\0",
            "$",
            "0x",
            "-$",
            "+-5",
            "-+$5",
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
            leading_values("20 30 6 2  Two Words ", [-1; 4]),
            ([20, 30, 6, 2], false, " Two Words")
        );
        assert_eq!(
            leading_values("40\t50", [-1; 4]),
            ([40, 50, -1, -1], true, "")
        );
        assert_eq!(
            leading_values("1 10\t5 1 name", [-1; 4]),
            ([-1, 5, 1, -1], true, "")
        );
        assert_eq!(leading_values("x", [0, 1]), ([0, 1], true, ""));
    }

    #[test]
    fn all_values_read_every_token() {
        assert_eq!(all_values("5\t$10 -2", -1), (vec![5, 16, -2], false));
        assert_eq!(all_values("5 x 7", 0), (vec![5, 0, 7], true));
        assert_eq!(all_values(" \t", -1), (vec![], false));
    }
}
