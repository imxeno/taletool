//! Row reading shared by the entity record readers and writers.
//!
//! Rows are split with the shared row tokenizer: the tag token first, then
//! value tokens from the rest of the row. Text fields are the trimmed rest.

use super::invalid;
use crate::Result;
use crate::gtd::row_tokens::{client_int, leading_values, split_token, tokens, trim};
use crate::gtd::{GtdWarning, parse_i32, warning};

/// A row split into its tag token and the untrimmed text after it.
#[derive(Debug, Clone, Copy)]
pub(super) struct TaggedRow<'a> {
    pub(super) tag: &'a str,
    pub(super) rest: &'a str,
}

impl<'a> TaggedRow<'a> {
    /// Splits a row, or returns `None` for a blank row.
    pub(super) fn parse(line: &'a str) -> Option<Self> {
        let (tag, rest) = split_token(line);
        (!tag.is_empty()).then_some(Self { tag, rest })
    }

    /// Text field value: the trimmed rest of the row.
    pub(super) fn text(&self) -> &'a str {
        trim(self.rest)
    }
}

/// How the client converts one value of a numeric row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Value {
    /// An integer, or the given default for a token that is not one.
    Int(i32),
    /// A boolean, false for a token that is not one.
    Bool,
}

/// Converts a token the client reads as a boolean: a number is true unless it
/// is zero, and `True` and `False` match in any letter case. A decimal integer
/// keeps its value, which the client reads the same way. `None` marks text for
/// which the client stores false.
fn client_bool(token: &str) -> Option<i32> {
    if let Some(value) = parse_i32(token) {
        return Some(value);
    }
    let flag = match client_number_is_nonzero(token) {
        Some(nonzero) => nonzero,
        None if token.eq_ignore_ascii_case("true") => true,
        None if token.eq_ignore_ascii_case("false") => false,
        None => return None,
    };
    Some(i32::from(flag))
}

/// Reads a token as the client's floating-point numbers: spaces, an optional
/// sign, digits with an optional `.` fraction, an optional exponent, and
/// spaces. Returns whether the number is not zero, or `None` for text that is
/// not a number.
fn client_number_is_nonzero(token: &str) -> Option<bool> {
    let text = token.trim_matches(' ');
    let text = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (mantissa, exponent) = match text.find(['E', 'e']) {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |text: &str| text.bytes().all(|byte| byte.is_ascii_digit());
    if mantissa.is_empty() || !digits(whole) || !digits(fraction) {
        return None;
    }
    if let Some(exponent) = exponent {
        // The client stops reading exponent digits once the exponent
        // reaches 500, so any further digit is unread text.
        let mut value = 0;
        for byte in exponent
            .strip_prefix(['+', '-'])
            .unwrap_or(exponent)
            .bytes()
        {
            if !byte.is_ascii_digit() || value >= 500 {
                return None;
            }
            value = value * 10 + u32::from(byte - b'0');
        }
    }
    Some(mantissa.bytes().any(|byte| matches!(byte, b'1'..=b'9')))
}

/// Collects the warnings for one entity record.
pub(super) struct RowReader {
    record: &'static str,
    warnings: Vec<GtdWarning>,
}

impl RowReader {
    pub(super) fn new(record: &'static str) -> Self {
        Self {
            record,
            warnings: Vec::new(),
        }
    }

    pub(super) fn warn(&mut self, row: usize, message: impl Into<String>) {
        self.warnings.push(warning(row, message));
    }

    /// Splits `line` and returns the tag the client reads it as. Comment
    /// rows and bare `END` and `~` rows are ignored silently; other rows the
    /// client ignores, and tags it reads under another name, are reported.
    pub(super) fn read<'a>(
        &mut self,
        row: usize,
        line: &'a str,
        read_as: fn(&str) -> Option<&'static str>,
    ) -> Option<(&'static str, TaggedRow<'a>)> {
        let tagged = TaggedRow::parse(line)?;
        let Some(tag) = read_as(tagged.tag) else {
            let framing = tagged.tag.starts_with('#')
                || (matches!(tagged.tag, "END" | "~") && tagged.text().is_empty());
            if !framing {
                self.warn(row, format!("unrecognized {} row", self.record));
            }
            return None;
        };
        if tag != tagged.tag {
            self.renamed(row, tagged.tag, tag);
        }
        Some((tag, tagged))
    }

    /// Reports a tag the client reads under another name.
    pub(super) fn renamed(&mut self, row: usize, tag: &str, read_as: &str) {
        self.warn(
            row,
            format!("{} tag {tag} is read as {read_as}", self.record),
        );
    }

    /// Reports a row that replaces what an earlier row stored.
    pub(super) fn repeated(&mut self, row: usize, tag: &str) {
        self.warn(
            row,
            format!(
                "repeated {} {tag} row replaces the earlier one",
                self.record
            ),
        );
    }

    /// Stores a row's values, reporting a row that replaces different values
    /// stored by an earlier row.
    pub(super) fn store<T: PartialEq>(
        &mut self,
        row: usize,
        tag: &str,
        field: &mut Option<T>,
        values: T,
    ) {
        if field.as_ref().is_some_and(|earlier| *earlier != values) {
            self.repeated(row, tag);
        }
        *field = Some(values);
    }

    /// Stores a row's text, reporting a row that replaces different text.
    pub(super) fn replace_text(&mut self, row: usize, tag: &str, field: &mut String, text: &str) {
        if !field.is_empty() && field != text {
            self.repeated(row, tag);
        }
        *field = text.to_owned();
    }

    /// Stores a text row's text like [`Self::replace_text`]. The client keeps
    /// the earlier text when a repeated row has none.
    pub(super) fn set_text(&mut self, row: usize, tag: &str, field: &mut String, text: &str) {
        if !text.is_empty() {
            self.replace_text(row, tag, field, text);
        }
    }

    /// Returns the entry a row belongs to. Rows before the first `VNUM` fill
    /// a record the client discards.
    pub(super) fn entry<'e, T>(
        &mut self,
        row: usize,
        current: &'e mut Option<T>,
    ) -> Option<&'e mut T> {
        if current.is_none() {
            self.warn(
                row,
                format!("{} row before the first VNUM has no entry", self.record),
            );
        }
        current.as_mut()
    }

    /// Reads every value of a numeric row with the client's conversion for
    /// each position. A token the client cannot convert takes the default the
    /// client stores instead, and the row is reported.
    pub(super) fn values(
        &mut self,
        row: usize,
        tag: &str,
        tagged: &TaggedRow,
        conversion: fn(&str, usize) -> Value,
    ) -> Vec<i32> {
        let mut defaulted = false;
        let values = tokens(tagged.rest)
            .enumerate()
            .map(|(position, token)| {
                let (value, default) = match conversion(tag, position) {
                    Value::Int(default) => (client_int(token), default),
                    Value::Bool => (client_bool(token), 0),
                };
                value.unwrap_or_else(|| {
                    defaulted = true;
                    default
                })
            })
            .collect::<Vec<_>>();
        if defaulted {
            self.malformed(row, tag, &values);
        }
        values
    }

    /// Reads the single value of a `VNUM`-like row with the client's default.
    /// A missing, non-numeric, or extra token is reported.
    pub(super) fn scalar(
        &mut self,
        row: usize,
        tag: &str,
        tagged: &TaggedRow,
        default: i32,
    ) -> i32 {
        let ([value], defaulted, rest) = leading_values(tagged.rest, [default]);
        if defaulted || !rest.is_empty() {
            self.malformed(row, tag, &[value]);
        }
        value
    }

    /// Reports a row whose leading values were replaced with the client's
    /// defaults or whose extra tokens were dropped.
    pub(super) fn malformed(&mut self, row: usize, tag: &str, stored: &[i32]) {
        let stored = stored
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        self.warn(
            row,
            format!(
                "malformed {} {tag} row stored as {tag} {stored}",
                self.record
            ),
        );
    }

    pub(super) fn finish(self) -> Vec<GtdWarning> {
        self.warnings
    }
}

/// Checks that `text` reads back unchanged from a `TAG<TAB>text` row.
pub(super) fn check_row_text(text: &str, field: &str) -> Result<()> {
    if text.contains(['\r', '\n']) {
        return invalid(format!("{field} contains a line break"));
    }
    if trim(text) != text {
        return invalid(format!(
            "{field} starts or ends with spaces or control characters that the client trims"
        ));
    }
    Ok(())
}

/// Checks that `text` reads back unchanged as the rest of a row after a value
/// token. The client keeps leading whitespace there but trims the row end.
pub(super) fn check_rest_text(text: &str, field: &str) -> Result<()> {
    if text.contains(['\r', '\n']) {
        return invalid(format!("{field} contains a line break"));
    }
    if text.trim_end_matches(|c: char| c <= ' ') != text {
        return invalid(format!(
            "{field} ends with spaces or control characters that the client trims"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_split_at_the_first_tab_before_any_space() {
        let row = TaggedRow::parse(" DESC x\tsp  y ").unwrap();
        assert_eq!(row.tag, "DESC x");
        assert_eq!(row.text(), "sp  y");

        let row = TaggedRow::parse("NAME A  B").unwrap();
        assert_eq!(row.tag, "NAME");
        assert_eq!(row.text(), "A  B");

        assert!(TaggedRow::parse(" \t ").is_none());
    }

    #[test]
    fn numeric_rows_store_the_client_conversion_of_each_token() {
        let conversion = |_: &str, position: usize| match position {
            0 => Value::Int(-1),
            1 => Value::Int(0),
            _ => Value::Bool,
        };
        let mut rows = RowReader::new("monster");
        let values = |rows: &mut RowReader, line: &str| {
            let tagged = TaggedRow::parse(line).unwrap();
            rows.values(1, "ETC", &tagged, conversion)
        };

        assert_eq!(values(&mut rows, "ETC $10 -$1F 5 0"), [16, -31, 5, 0]);
        assert_eq!(
            values(&mut rows, "ETC\t7\t8 TRUE false 1.5 .0e+12"),
            [7, 8, 1, 0, 1, 0]
        );
        assert!(rows.warnings.is_empty());
        assert_eq!(values(&mut rows, "ETC\tx\ty\t$1\t1 \t2"), [-1, 0, 0, 1, 2]);
        assert_eq!(
            rows.finish(),
            [warning(
                1,
                "malformed monster ETC row stored as ETC -1 0 0 1 2"
            )]
        );
    }

    #[test]
    fn booleans_follow_the_client_conversion() {
        for (token, value) in [
            ("-7", Some(-7)),
            ("+0", Some(0)),
            ("True", Some(1)),
            ("fALSE", Some(0)),
            ("1 ", Some(1)),
            (" -0.00 ", Some(0)),
            ("2.", Some(1)),
            (".", Some(0)),
            ("0.001", Some(1)),
            ("1e", Some(1)),
            ("0E-5", Some(0)),
            ("9e0000000499", Some(1)),
            ("1e500", Some(1)),
            ("99999999999", Some(1)),
            ("", None),
            ("-", None),
            ("$1", None),
            ("0x1", None),
            ("e5", None),
            ("1.2.3", None),
            ("1e5000", None),
            ("1 e2", None),
            ("\t1", None),
            (" true", None),
            ("yes", None),
        ] {
            assert_eq!(client_bool(token), value, "{token:?}");
        }
    }

    #[test]
    fn row_text_checks_follow_client_trimming() {
        assert!(check_row_text("a  b\tc", "text").is_ok());
        assert!(check_row_text(" a", "text").is_err());
        assert!(check_row_text("a\t", "text").is_err());
        assert!(check_row_text("a\nb", "text").is_err());
        assert!(check_rest_text("  lead", "text").is_ok());
        assert!(check_rest_text("trail ", "text").is_err());
    }
}
