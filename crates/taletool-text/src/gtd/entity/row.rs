//! Row splitting shared by the entity record readers and writers.
//!
//! The client trims each row and splits off its tag token at the first tab,
//! or at the first space when the row has no tab. Value tokens are split from
//! the rest of the row the same way, and text fields are the trimmed rest.

use super::invalid;
use crate::Result;
use crate::gtd::{GtdWarning, parse_i32, warning};

/// Removes leading and trailing spaces and control characters, as the client
/// trims rows and text fields.
pub(super) fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| c <= ' ')
}

/// Splits the leading token from `text` and returns it with the untrimmed
/// text after its delimiter.
pub(super) fn split_token(text: &str) -> (&str, &str) {
    let text = trim(text);
    let delimiter = if text.contains('\t') { '\t' } else { ' ' };
    text.split_once(delimiter).unwrap_or((text, ""))
}

/// A row split into its tag token and the text after it.
#[derive(Debug, Clone, Copy)]
pub(super) struct TaggedRow<'a> {
    pub(super) tag: &'a str,
    rest: &'a str,
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

    pub(super) fn tokens(&self) -> Tokens<'a> {
        Tokens { rest: self.rest }
    }

    /// Reads `N` leading values with the client's defaults for missing or
    /// non-decimal tokens. Also returns whether every value was a decimal
    /// token, and the untrimmed text after those tokens.
    pub(super) fn leading<const N: usize>(&self, defaults: [i32; N]) -> ([i32; N], bool, &'a str) {
        let mut tokens = self.tokens();
        let mut exact = true;
        let values = defaults.map(|default| {
            tokens.next().and_then(parse_i32).unwrap_or_else(|| {
                exact = false;
                default
            })
        });
        (values, exact, tokens.rest())
    }
}

/// Value tokens split from the rest of a row.
#[derive(Debug, Clone)]
pub(super) struct Tokens<'a> {
    rest: &'a str,
}

impl<'a> Tokens<'a> {
    /// The untrimmed text after the tokens read so far.
    pub(super) fn rest(&self) -> &'a str {
        self.rest
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let (token, rest) = split_token(self.rest);
        if token.is_empty() {
            return None;
        }
        self.rest = rest;
        Some(token)
    }
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

    /// Reads every value of a numeric row, or reports a row with a
    /// non-decimal value.
    pub(super) fn values(&mut self, row: usize, tag: &str, tagged: &TaggedRow) -> Option<Vec<i32>> {
        let values: Option<Vec<i32>> = tagged.tokens().map(parse_i32).collect();
        if values.is_none() {
            self.warn(
                row,
                format!("non-decimal value in {} {tag} row", self.record),
            );
        }
        values
    }

    /// Reads the single value of a `VNUM`-like row with the client's default.
    /// A missing, non-decimal, or extra token is reported.
    pub(super) fn scalar(
        &mut self,
        row: usize,
        tag: &str,
        tagged: &TaggedRow,
        default: i32,
    ) -> i32 {
        let ([value], exact, rest) = tagged.leading([default]);
        if !exact || !rest.is_empty() {
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
    fn tokens_keep_untrimmed_rest_after_a_value() {
        let row = TaggedRow::parse("LINEDESC 0  two  spaces").unwrap();
        let ([count], exact, rest) = row.leading([7]);
        assert_eq!((count, exact, rest), (0, true, " two  spaces"));

        let row = TaggedRow::parse("LINEDESC\t0\t\tlead").unwrap();
        let mut tokens = row.tokens();
        assert_eq!(tokens.next(), Some("0"));
        assert_eq!(tokens.rest(), "\tlead");
    }

    #[test]
    fn leading_values_use_defaults_for_missing_and_invalid_tokens() {
        let row = TaggedRow::parse("VNUM junk").unwrap();
        assert_eq!(row.leading([-1, 0]), ([-1, 0], false, ""));

        let row = TaggedRow::parse("VNUM 5 10 extra").unwrap();
        assert_eq!(row.leading([-1, 0]), ([5, 10], true, "extra"));
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
