//! Models for the entity tables in `NSgtdData.NOS`.

use serde::{Deserialize, Serialize};

use super::row_tokens::{all_values, client_int, leading_values, trim};
use super::{
    ParsedGtd, fields, is_ignored_line, parse_i32, push_text, push_values, values, warning,
};
use crate::{Result, TextError};

mod row;

use row::{RowReader, TaggedRow, Value, check_rest_text, check_row_text};

macro_rules! document {
    ($name:ident, $entry:ty) => {
        #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub entries: Vec<$entry>,
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActDataRow {
    pub vnum: i32,
    pub act_vnum: i32,
    pub part: i32,
    pub max_ts: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActTitleRow {
    pub act_vnum: i32,
    pub title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActDescriptionDocument {
    pub data: Vec<ActDataRow>,
    pub titles: Vec<ActTitleRow>,
}

pub fn decode_act_description(text: &str) -> Result<ParsedGtd<ActDescriptionDocument>> {
    let mut document = ActDescriptionDocument::default();
    let mut warnings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        if is_ignored_line(line) || matches!(line.trim(), "end" | "~") {
            continue;
        }
        let f = fields(line);
        match f.first().copied() {
            Some("Data") if f.len() == 5 => match values(&f[1..]) {
                Some(v) => document.data.push(ActDataRow {
                    vnum: v[0],
                    act_vnum: v[1],
                    part: v[2],
                    max_ts: v[3],
                }),
                None => warnings.push(warning(row, "invalid Data row")),
            },
            Some("A") if f.len() >= 3 => match parse_i32(f[1]) {
                Some(act_vnum) => document.titles.push(ActTitleRow {
                    act_vnum,
                    title: f[2..].join(" "),
                }),
                None => warnings.push(warning(row, "invalid A row")),
            },
            _ => warnings.push(warning(row, "unrecognized act-description row")),
        }
    }
    Ok(ParsedGtd { document, warnings })
}

pub fn encode_act_description(document: &ActDescriptionDocument) -> Result<String> {
    let mut out = String::new();
    for row in &document.data {
        push_values(
            &mut out,
            "Data",
            &[row.vnum, row.act_vnum, row.part, row.max_ts],
        );
    }
    out.push_str("end\n");
    for row in &document.titles {
        push_text(&mut out, "A", &format!("{}\t{}", row.act_vnum, row.title));
    }
    out.push_str("~\n");
    Ok(out)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasicCardEntry {
    pub vnum: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<i32>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Vec<i32>>,
    /// `SUBJ0` through `SUBJ4` text by slot; empty without a row.
    pub subject_slots: [String; BASIC_CARD_SLOTS],
    /// `LISTk-1` and `LISTk-2` text of slot `k - 1`; empty without a row.
    pub list_slots: [[String; 2]; BASIC_CARD_SLOTS],
    /// `SUBJ` and `LIST` rows whose index the client ignores, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignored_rows: Vec<BasicCardIgnoredRow>,
}
document!(BasicCardDocument, BasicCardEntry);

/// A `SUBJ` or `LIST` row kept as its source tag and text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasicCardIgnoredRow {
    pub tag: String,
    pub text: String,
}

/// Number of subject and list slots the client stores per BCard entry.
const BASIC_CARD_SLOTS: usize = 5;

/// A BCard text slot that a `SUBJ` or `LIST` row fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BasicCardSlot {
    /// `SUBJn` fills subject slot `n`.
    Subject(usize),
    /// `LISTk-m` fills template `m - 1` of slot `k - 1`.
    List(usize, usize),
}

impl BasicCardSlot {
    /// Reads an ASCII tag beginning with `S` or `L` as the client does. The
    /// index starts after the fourth character, whatever those characters
    /// are. Returns `None` for an index outside the client's slots.
    fn parse(tag: &str) -> Option<Self> {
        let one_based = |index: &str, count: usize| {
            client_int(index)?
                .checked_sub(1)
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < count)
        };
        match tag.as_bytes().first()? {
            b'S' => {
                let slot = usize::try_from(client_int(tag.get(4..)?)?).ok()?;
                (slot < BASIC_CARD_SLOTS).then_some(Self::Subject(slot))
            }
            b'L' => {
                let dash = tag.find('-').filter(|dash| *dash > 4)?;
                Some(Self::List(
                    one_based(tag.get(4..dash)?, BASIC_CARD_SLOTS)?,
                    one_based(tag.get(dash + 1..)?, 2)?,
                ))
            }
            _ => None,
        }
    }

    fn tag(self) -> String {
        match self {
            Self::Subject(slot) => format!("SUBJ{slot}"),
            Self::List(slot, template) => format!("LIST{}-{}", slot + 1, template + 1),
        }
    }

    fn text_mut(self, entry: &mut BasicCardEntry) -> &mut String {
        match self {
            Self::Subject(slot) => &mut entry.subject_slots[slot],
            Self::List(slot, template) => &mut entry.list_slots[slot][template],
        }
    }
}

/// Maps a BCard tag to the row the client reads it as. Tags beginning with
/// `S` or `L` are `SUBJ` and `LIST` rows, read by [`BasicCardSlot::parse`].
fn basic_card_row(tag: &str) -> Option<&'static str> {
    Some(match tag.as_bytes()[0] {
        b'V' => "VNUM",
        b'I' => "ICON",
        b'N' => "NAME",
        b'D' => "DESC",
        _ => return None,
    })
}

pub fn decode_basic_card(text: &str) -> Result<ParsedGtd<BasicCardDocument>> {
    let mut rows = RowReader::new("BCard");
    let mut entries = Vec::new();
    let mut current: Option<BasicCardEntry> = None;
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        if let Some(tagged) =
            TaggedRow::parse(line).filter(|tagged| tagged.tag.starts_with(['S', 'L']))
        {
            let Some(entry) = rows.entry(row, &mut current) else {
                continue;
            };
            read_basic_card_text(&mut rows, row, tagged, entry);
            continue;
        }
        let Some((tag, tagged)) = rows.read(row, line, basic_card_row) else {
            continue;
        };
        if tag == "VNUM" {
            let vnum = rows.scalar(row, tag, &tagged, -1);
            entries.extend(current.replace(BasicCardEntry {
                vnum,
                ..BasicCardEntry::default()
            }));
            continue;
        }
        let Some(entry) = rows.entry(row, &mut current) else {
            continue;
        };
        match tag {
            "ICON" => entry.icon = Some(rows.scalar(row, tag, &tagged, -1)),
            "NAME" => {
                if !entry.name.is_empty() {
                    rows.warn(
                        row,
                        "repeated BCard NAME row; the client frees the earlier name, so its result is unreliable",
                    );
                }
                set_text(&mut entry.name, tagged.text());
            }
            "DESC" => {
                // The client reads only the first five values, and a
                // non-numeric value as 0.
                let (values, defaulted) = all_values(tagged.rest, 0);
                if defaulted {
                    rows.malformed(row, tag, &values);
                }
                // A repeated row replaces only the values it has.
                let description = entry.description.get_or_insert_default();
                for (index, value) in values.into_iter().enumerate() {
                    match description.get_mut(index) {
                        Some(earlier) => *earlier = value,
                        None => description.push(value),
                    }
                }
            }
            _ => unreachable!(),
        }
    }
    entries.extend(current);
    Ok(ParsedGtd {
        document: BasicCardDocument { entries },
        warnings: rows.finish(),
    })
}

/// Reads a `SUBJ` or `LIST` row into its slot, or keeps a row whose index
/// the client ignores.
fn read_basic_card_text(
    rows: &mut RowReader,
    row: usize,
    tagged: TaggedRow,
    entry: &mut BasicCardEntry,
) {
    let text = tagged.text();
    // The client finds the index by byte position in the encoded tag.
    match tagged
        .tag
        .is_ascii()
        .then(|| BasicCardSlot::parse(tagged.tag))
        .flatten()
    {
        Some(slot) => {
            let tag = slot.tag();
            if tag != tagged.tag {
                rows.renamed(row, tagged.tag, &tag);
            }
            set_text(slot.text_mut(entry), text);
        }
        None if is_ignored_slot_tag(tagged.tag) => {
            entry.ignored_rows.push(BasicCardIgnoredRow {
                tag: tagged.tag.to_owned(),
                text: text.to_owned(),
            });
        }
        None => rows.warn(
            row,
            format!(
                "BCard {} row is dropped because its slot index is not plain decimal",
                tagged.tag
            ),
        ),
    }
}

/// Whether `tag` is a `SUBJn` or `LISTk-m` tag with decimal indexes that
/// the client ignores because they select no slot.
fn is_ignored_slot_tag(tag: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let numbered = if let Some(index) = tag.strip_prefix("SUBJ") {
        digits(index)
    } else if let Some(indexes) = tag.strip_prefix("LIST") {
        indexes
            .split_once('-')
            .is_some_and(|(slot, template)| digits(slot) && digits(template))
    } else {
        false
    };
    numbered && BasicCardSlot::parse(tag).is_none()
}

pub fn encode_basic_card(document: &BasicCardDocument) -> Result<String> {
    let mut out = String::new();
    for e in &document.entries {
        check_row_text(&e.name, &format!("BCard entry {} name", e.vnum))?;
        push_values(&mut out, "VNUM", &[e.vnum]);
        if let Some(icon) = e.icon {
            push_values(&mut out, "ICON", &[icon]);
        }
        push_text(&mut out, "NAME", &e.name);
        push_optional_values(&mut out, "DESC", &e.description);
        for (slot, text) in e.subject_slots.iter().enumerate() {
            push_basic_card_text(&mut out, e.vnum, BasicCardSlot::Subject(slot), text)?;
        }
        for row in &e.ignored_rows {
            check_basic_card_ignored_row(row, e.vnum)?;
            push_text(&mut out, &row.tag, &row.text);
        }
        for (slot, templates) in e.list_slots.iter().enumerate() {
            for (template, text) in templates.iter().enumerate() {
                push_basic_card_text(&mut out, e.vnum, BasicCardSlot::List(slot, template), text)?;
            }
        }
        out.push_str("END\n");
    }
    Ok(out)
}

/// Writes a slot's text row. An empty slot needs no row.
fn push_basic_card_text(
    out: &mut String,
    vnum: i32,
    slot: BasicCardSlot,
    text: &str,
) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let tag = slot.tag();
    check_row_text(text, &format!("BCard entry {vnum} {tag}"))?;
    push_text(out, &tag, text);
    Ok(())
}

fn check_basic_card_ignored_row(row: &BasicCardIgnoredRow, vnum: i32) -> Result<()> {
    let tag = &row.tag;
    if !is_ignored_slot_tag(tag) {
        return invalid(format!(
            "BCard entry {vnum} ignored row {tag:?} must be a SUBJn or LISTk-m tag that selects no slot"
        ));
    }
    check_row_text(&row.text, &format!("BCard entry {vnum} {tag}"))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardEntry {
    pub vnum: i32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_stage: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_stage: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<Vec<i32>>,
    pub description: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardDocument {
    pub kits: Vec<Vec<String>>,
    pub extra_texts: Vec<String>,
    pub entries: Vec<CardEntry>,
}

/// Maps a Card tag to the row the client reads it as.
fn card_row(tag: &str) -> Option<&'static str> {
    Some(match tag.as_bytes()[0] {
        b'K' => "KIT",
        b'Z' => "Z_ETC",
        b'E' if tag == "EFFECT" => "EFFECT",
        b'V' => "VNUM",
        b'I' => "ICON",
        b'N' => "NAME",
        b'G' => "GROUP",
        b'S' => "STYLE",
        b'T' => "TIME",
        b'1' => "1ST",
        b'2' => "2ST",
        b'L' => "LAST",
        b'D' => "DESC",
        _ => return None,
    })
}

pub fn decode_card(text: &str) -> Result<ParsedGtd<CardDocument>> {
    let mut kits = vec![vec![String::new(); 5]; 3];
    let mut extra_texts = vec![String::new(); 20];
    let mut rows = RowReader::new("Card");
    let mut entries = Vec::new();
    let mut current = None;
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        let Some((tag, tagged)) = rows.read(row, line, card_row) else {
            continue;
        };
        match tag {
            "KIT" => {
                let ([kit, slot], defaulted, rest) = leading_values(tagged.rest, [0, 0]);
                if defaulted {
                    rows.malformed(row, tag, &[kit, slot]);
                }
                if (0..3).contains(&kit) && (0..5).contains(&slot) {
                    kits[kit as usize][slot as usize] = trim(rest).to_owned();
                } else {
                    rows.warn(row, "invalid KIT row");
                }
            }
            "Z_ETC" => {
                let ([slot], defaulted, rest) = leading_values(tagged.rest, [0]);
                if defaulted {
                    rows.malformed(row, tag, &[slot]);
                }
                if (0..20).contains(&slot) {
                    extra_texts[slot as usize] = trim(rest).to_owned();
                } else {
                    rows.warn(row, "invalid Z_ETC row");
                }
            }
            "VNUM" => {
                let vnum = rows.scalar(row, tag, &tagged, -1);
                entries.extend(current.replace(CardEntry {
                    vnum,
                    ..CardEntry::default()
                }));
            }
            _ => {
                let Some(entry) = rows.entry(row, &mut current) else {
                    continue;
                };
                match tag {
                    "NAME" => set_text(&mut entry.name, tagged.text()),
                    "DESC" => set_text(&mut entry.description, tagged.text()),
                    "ICON" => entry.icon = Some(rows.scalar(row, tag, &tagged, -1)),
                    _ => {
                        let values = rows.values(row, tag, &tagged, |_, _| Value::Int(-1));
                        let field = match tag {
                            "GROUP" => &mut entry.group,
                            "STYLE" => &mut entry.style,
                            "EFFECT" => {
                                // EFFECT's second value replaces the icon set
                                // by an earlier ICON row.
                                entry.icon = None;
                                &mut entry.effect
                            }
                            "TIME" => &mut entry.time,
                            "1ST" => &mut entry.first_stage,
                            "2ST" => &mut entry.second_stage,
                            "LAST" => &mut entry.last,
                            _ => unreachable!(),
                        };
                        *field = Some(values);
                    }
                }
            }
        }
    }
    entries.extend(current);
    Ok(ParsedGtd {
        document: CardDocument {
            kits,
            extra_texts,
            entries,
        },
        warnings: rows.finish(),
    })
}

pub fn encode_card(d: &CardDocument) -> Result<String> {
    exact(&d.kits, 3, "Card KIT")?;
    exact(&d.extra_texts, 20, "Card Z_ETC")?;
    let mut out = String::from("END\n");
    for (i, r) in d.kits.iter().enumerate() {
        exact(r, 5, "Card KIT row")?;
        for (j, s) in r.iter().enumerate() {
            check_row_text(s, &format!("Card KIT {i} {j} text"))?;
            push_text(&mut out, "KIT", &format!("{i}\t{j}\t{s}"))
        }
    }
    for (i, s) in d.extra_texts.iter().enumerate() {
        check_row_text(s, &format!("Card Z_ETC {i} text"))?;
        push_text(&mut out, "Z_ETC", &format!("{i}\t{s}"))
    }
    for e in &d.entries {
        check_row_text(&e.name, &format!("Card entry {} name", e.vnum))?;
        check_row_text(&e.description, &format!("Card entry {} DESC", e.vnum))?;
        push_values(&mut out, "VNUM", &[e.vnum]);
        push_text(&mut out, "NAME", &e.name);
        push_optional_values(&mut out, "GROUP", &e.group);
        push_optional_values(&mut out, "STYLE", &e.style);
        push_optional_values(&mut out, "EFFECT", &e.effect);
        if let Some(icon) = e.icon {
            push_values(&mut out, "ICON", &[icon]);
        }
        push_optional_values(&mut out, "TIME", &e.time);
        push_optional_values(&mut out, "1ST", &e.first_stage);
        push_optional_values(&mut out, "2ST", &e.second_stage);
        push_optional_values(&mut out, "LAST", &e.last);
        push_text(&mut out, "DESC", &e.description);
        out.push_str("END\n")
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemEntry {
    pub vnum: i32,
    pub price: i32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<Vec<i32>>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub item_type: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flags: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffs: Option<Vec<Vec<i32>>>,
    pub line_desc_count: i32,
    /// Rows read after a positive `LINEDESC` count, joined with line feeds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Text after a non-positive count on the `LINEDESC` row itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_description: Option<String>,
}
document!(ItemDocument, ItemEntry);

/// Position of the FLAG value that makes the client append the signed-item
/// label to the name loaded so far.
const ITEM_SIGNED_FLAG: usize = 22;

/// Maps an Item tag to the row the client reads it as.
fn item_row(tag: &str) -> Option<&'static str> {
    Some(match tag.as_bytes()[0] {
        b'V' => "VNUM",
        b'N' => "NAME",
        b'I' => "INDEX",
        b'T' => "TYPE",
        b'F' => "FLAG",
        b'D' => "DATA",
        b'B' => "BUFF",
        b'L' => "LINEDESC",
        _ => return None,
    })
}

/// Whether an INDEX row adds the item to one of the client's four type
/// lists. The client keeps the type in 16 bits and maps 8, 9, and 10 to 0,
/// 1, and 2.
fn item_index_lists_type(values: &[i32]) -> bool {
    let item_type = match values.first().map_or(-1, |value| *value as i16) {
        8 => 0,
        9 => 1,
        10 => 2,
        item_type => item_type,
    };
    (0..4).contains(&item_type)
}

/// How the client converts each value of an Item row. All but the first
/// `FLAG` value are flags that default to 0.
fn item_value(tag: &str, position: usize) -> Value {
    Value::Int(if tag == "FLAG" && (1..25).contains(&position) {
        0
    } else {
        -1
    })
}

/// Counts the signed-item labels the client appends to an Item name. Each
/// FLAG row with the signed-item flag appends one, and a NAME row with text
/// replaces the name. Packing writes NAME before a single FLAG row, so the
/// packed name has at most the last FLAG row's label.
#[derive(Default)]
struct SignedLabels {
    /// Labels appended after the last NAME row with text.
    appended: usize,
    /// Whether the last FLAG row sets the signed-item flag.
    last_flag: bool,
    /// The last FLAG or NAME row that changed the labels.
    row: usize,
}

impl SignedLabels {
    fn flag(&mut self, row: usize, signed: bool) {
        self.appended += usize::from(signed);
        self.last_flag = signed;
        self.row = row;
    }

    fn name(&mut self, row: usize) {
        self.appended = 0;
        self.row = row;
    }

    /// Reports an entry whose packed name would have a different number of
    /// labels, and starts counting for the next entry.
    fn finish(&mut self, rows: &mut RowReader) {
        let packed = usize::from(self.last_flag);
        if self.appended != packed {
            rows.warn(
                self.row,
                format!(
                    "Item name has {} signed-item labels in the client but {packed} after packing, which writes NAME before one FLAG row",
                    self.appended
                ),
            );
        }
        *self = Self::default();
    }
}

/// The client keeps Item and Skill description counts in a 16-bit word and
/// reads description rows only when its signed value is positive.
fn description_count_is_positive(count: i32) -> bool {
    count as i16 > 0
}

pub fn decode_item(text: &str) -> Result<ParsedGtd<ItemDocument>> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut rows = RowReader::new("Item");
    let mut entries = Vec::new();
    let mut current: Option<ItemEntry> = None;
    let mut labels = SignedLabels::default();
    // Whether an earlier INDEX row of the entry added it to a type list.
    let mut listed = false;
    // The client sets its append limit only for a description whose first
    // row is not END, and later scans reuse it.
    let mut append_limit = 0;
    let mut next = 0;
    while let Some(line) = lines.get(next) {
        let row = next + 1;
        next += 1;
        let Some((tag, tagged)) = rows.read(row, line, item_row) else {
            continue;
        };
        match tag {
            "VNUM" => {
                let ([vnum, price], defaulted, rest) = leading_values(tagged.rest, [-1, 0]);
                if defaulted || !rest.is_empty() {
                    rows.malformed(row, tag, &[vnum, price]);
                }
                labels.finish(&mut rows);
                listed = false;
                entries.extend(current.replace(ItemEntry {
                    vnum,
                    price,
                    ..ItemEntry::default()
                }));
            }
            "LINEDESC" => {
                let ([count], defaulted, rest) = leading_values(tagged.rest, [0]);
                if defaulted {
                    rows.malformed(row, tag, &[count]);
                }
                let mut description = None;
                let mut inline_description = None;
                if description_count_is_positive(count) {
                    if !rest.is_empty() {
                        rows.warn(row, "text after a positive Item LINEDESC count is ignored");
                    }
                    if let Some(first) = lines.get(next) {
                        next += 1;
                        let mut text = String::new();
                        if trim(first) != "END" {
                            text.push_str(trim(first));
                            append_limit = 100;
                        }
                        for _ in 0..append_limit {
                            let Some(line) = lines.get(next) else {
                                break;
                            };
                            if line.starts_with('#') {
                                break;
                            }
                            next += 1;
                            if trim(line) == "END" {
                                break;
                            }
                            text.push('\n');
                            text.push_str(trim(line));
                        }
                        description = (!text.is_empty()).then_some(text);
                    }
                } else if !rest.is_empty() {
                    inline_description = Some(rest.to_owned());
                }
                if let Some(entry) = rows.entry(row, &mut current) {
                    entry.line_desc_count = count;
                    entry.description = description;
                    entry.inline_description = inline_description;
                }
            }
            _ => {
                let Some(entry) = rows.entry(row, &mut current) else {
                    continue;
                };
                if tag == "NAME" {
                    if !tagged.text().is_empty() {
                        labels.name(row);
                    }
                    set_text(&mut entry.name, tagged.text());
                    continue;
                }
                let values = rows.values(row, tag, &tagged, item_value);
                match tag {
                    "INDEX" => {
                        if listed {
                            rows.warn(
                                row,
                                "repeated Item INDEX row; the client adds the item to a type list for each INDEX row, but packing writes only the last one",
                            );
                        }
                        listed |= item_index_lists_type(&values);
                        entry.index = Some(values);
                    }
                    "TYPE" => entry.item_type = Some(values),
                    "FLAG" => {
                        let signed = values.get(ITEM_SIGNED_FLAG).is_some_and(|flag| *flag != 0);
                        labels.flag(row, signed);
                        entry.flags = Some(values);
                    }
                    "DATA" => entry.data = Some(values),
                    "BUFF" => entry.buffs = Some(chunks(values, 5)),
                    _ => unreachable!(),
                }
            }
        }
    }
    labels.finish(&mut rows);
    entries.extend(current);
    Ok(ParsedGtd {
        document: ItemDocument { entries },
        warnings: rows.finish(),
    })
}

pub fn encode_item(d: &ItemDocument) -> Result<String> {
    let mut out = String::new();
    for e in &d.entries {
        check_row_text(&e.name, &format!("Item entry {} name", e.vnum))?;
        push_values(&mut out, "VNUM", &[e.vnum, e.price]);
        push_text(&mut out, "NAME", &e.name);
        push_optional_values(&mut out, "INDEX", &e.index);
        push_optional_values(&mut out, "TYPE", &e.item_type);
        push_optional_values(&mut out, "FLAG", &e.flags);
        push_optional_values(&mut out, "DATA", &e.data);
        push_optional_groups(&mut out, "BUFF", &e.buffs, 5)?;
        push_item_description(&mut out, e)?;
        out.push_str("END\n")
    }
    Ok(out)
}

fn push_item_description(out: &mut String, e: &ItemEntry) -> Result<()> {
    let count = e.line_desc_count;
    if !description_count_is_positive(count) {
        if e.description
            .as_deref()
            .is_some_and(|text| !text.is_empty())
        {
            return invalid(format!(
                "Item entry {} has a description, but its LINEDESC count {count} is not positive; the client reads only inline_description from the LINEDESC row",
                e.vnum
            ));
        }
        match e.inline_description.as_deref() {
            Some(text) if !text.is_empty() => {
                check_rest_text(text, &format!("Item entry {} inline_description", e.vnum))?;
                push_text(out, "LINEDESC", &format!("{count}\t{text}"));
            }
            _ => push_values(out, "LINEDESC", &[count]),
        }
        return Ok(());
    }

    if e.inline_description
        .as_deref()
        .is_some_and(|text| !text.is_empty())
    {
        return invalid(format!(
            "Item entry {} has inline_description, but its LINEDESC count {count} is positive; the client reads only the rows after LINEDESC",
            e.vnum
        ));
    }
    // A blank first row keeps an empty description from reading later rows.
    let description = e.description.as_deref().unwrap_or_default();
    let lines = description.split('\n').collect::<Vec<_>>();
    if lines.len() > 101 {
        return invalid("Item description cannot contain more than 101 physical rows");
    }
    for (index, line) in lines.iter().enumerate() {
        check_row_text(
            line,
            &format!("Item entry {} description row {}", e.vnum, index + 1),
        )?;
        if *line == "END" {
            return invalid("Item description contains its END boundary");
        }
    }
    push_values(out, "LINEDESC", &[count]);
    push_description_rows(out, &lines);
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonsterEntry {
    pub vnum: i32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub race: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attributes: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hp_mp: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experience: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_attack: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etc: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pet_info: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_skills: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon_info: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub armor_info: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub armor: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<Vec<i32>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partner: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic: Option<Vec<Vec<i32>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cards: Option<Vec<Vec<i32>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<Vec<i32>>>,
}
document!(MonsterDocument, MonsterEntry);

impl MonsterEntry {
    fn values_mut(&mut self, tag: &str) -> &mut Option<Vec<i32>> {
        match tag {
            "LEVEL" => &mut self.level,
            "RACE" => &mut self.race,
            "ATTRIB" => &mut self.attributes,
            "HP/MP" => &mut self.hp_mp,
            "EXP" => &mut self.experience,
            "PREATT" => &mut self.pre_attack,
            "SETTING" => &mut self.settings,
            "ETC" => &mut self.etc,
            "PETINFO" => &mut self.pet_info,
            "EFF" => &mut self.effects,
            "ZSKILL" => &mut self.z_skills,
            "WINFO" => &mut self.weapon_info,
            "WEAPON" => &mut self.weapon,
            "AINFO" => &mut self.armor_info,
            "ARMOR" => &mut self.armor,
            "PARTNER" => &mut self.partner,
            "MODE" => &mut self.mode,
            _ => unreachable!(),
        }
    }
}

/// Monster tags the client matches exactly. It never reads `EFF`, `PARTNER`,
/// or `ITEM`; they are kept as source rows.
const MONSTER_EXACT_TAGS: [&str; 14] = [
    "ATTRIB", "AINFO", "ARMOR", "WINFO", "WEAPON", "EXP", "ETC", "EFF", "PREATT", "PETINFO",
    "PARTNER", "SETTING", "SKILL", "ITEM",
];

/// Maps a monster tag to the row the client reads it as.
fn monster_row(tag: &str) -> Option<&'static str> {
    Some(match tag.as_bytes()[0] {
        b'V' => "VNUM",
        b'N' => "NAME",
        b'L' => "LEVEL",
        b'R' => "RACE",
        b'H' => "HP/MP",
        b'Z' => "ZSKILL",
        b'B' => "BASIC",
        b'C' => "CARD",
        b'M' => "MODE",
        _ => return MONSTER_EXACT_TAGS.into_iter().find(|exact| *exact == tag),
    })
}

/// How the client converts each value of a monster row. Values it skips or
/// never reads take -1.
fn monster_value(tag: &str, position: usize) -> Value {
    match (tag, position) {
        ("ETC", 2..6) => Value::Bool,
        ("SETTING", 3) => Value::Int(1),
        ("SETTING", 4) | ("ZSKILL", 2..5) | ("WINFO", 2) | ("AINFO", 1) => Value::Int(0),
        _ => Value::Int(-1),
    }
}

pub fn decode_monster(text: &str) -> Result<ParsedGtd<MonsterDocument>> {
    let mut rows = RowReader::new("monster");
    let mut entries = Vec::new();
    let mut current = None;
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        let Some((tag, tagged)) = rows.read(row, line, monster_row) else {
            continue;
        };
        if tag == "VNUM" {
            let vnum = rows.scalar(row, tag, &tagged, -1);
            entries.extend(current.replace(MonsterEntry {
                vnum,
                ..MonsterEntry::default()
            }));
            continue;
        }
        let Some(entry) = rows.entry(row, &mut current) else {
            continue;
        };
        if tag == "NAME" {
            set_text(&mut entry.name, tagged.text());
            continue;
        }
        let values = rows.values(row, tag, &tagged, monster_value);
        match tag {
            "SKILL" => entry.skills = Some(chunks(values, 3)),
            "BASIC" => entry.basic = Some(chunks(values, 5)),
            "CARD" => entry.cards = Some(chunks(values, 5)),
            "ITEM" => entry.items = Some(chunks(values, 3)),
            _ => *entry.values_mut(tag) = Some(values),
        }
    }
    entries.extend(current);
    Ok(ParsedGtd {
        document: MonsterDocument { entries },
        warnings: rows.finish(),
    })
}

pub fn encode_monster(d: &MonsterDocument) -> Result<String> {
    let mut out = String::new();
    for e in &d.entries {
        check_row_text(&e.name, &format!("monster entry {} name", e.vnum))?;
        push_values(&mut out, "VNUM", &[e.vnum]);
        push_text(&mut out, "NAME", &e.name);
        let rows: [(&str, &Option<Vec<i32>>); 15] = [
            ("LEVEL", &e.level),
            ("RACE", &e.race),
            ("ATTRIB", &e.attributes),
            ("HP/MP", &e.hp_mp),
            ("EXP", &e.experience),
            ("PREATT", &e.pre_attack),
            ("SETTING", &e.settings),
            ("ETC", &e.etc),
            ("PETINFO", &e.pet_info),
            ("EFF", &e.effects),
            ("ZSKILL", &e.z_skills),
            ("WINFO", &e.weapon_info),
            ("WEAPON", &e.weapon),
            ("AINFO", &e.armor_info),
            ("ARMOR", &e.armor),
        ];
        for (tag, values) in rows {
            push_optional_values(&mut out, tag, values);
        }
        push_optional_groups(&mut out, "SKILL", &e.skills, 3)?;
        push_optional_values(&mut out, "PARTNER", &e.partner);
        push_optional_groups(&mut out, "BASIC", &e.basic, 5)?;
        push_optional_groups(&mut out, "CARD", &e.cards, 5)?;
        push_optional_values(&mut out, "MODE", &e.mode);
        push_optional_groups(&mut out, "ITEM", &e.items, 3)?;
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillDescription {
    pub declared_count: i32,
    /// Rows read after a positive count.
    pub lines: Vec<String>,
    /// Text after a non-positive count on the `Z_DESC` row itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_text: Option<String>,
}

impl SkillDescription {
    /// The description text the client loads, with rows joined by line feeds.
    fn text(&self) -> String {
        if description_count_is_positive(self.declared_count) {
            self.lines.join("\n")
        } else {
            self.inline_text.clone().unwrap_or_default()
        }
    }

    /// This description's text under another count, if a single `Z_DESC`
    /// row with that count can carry it.
    fn with_count(&self, declared_count: i32) -> Option<Self> {
        let text = self.text();
        if description_count_is_positive(declared_count) {
            // Rows after a positive count are trimmed.
            let lines = text.split('\n').map(str::to_owned).collect::<Vec<_>>();
            lines.iter().all(|line| trim(line) == line).then_some(Self {
                declared_count,
                lines,
                inline_text: None,
            })
        } else {
            (!text.contains('\n')).then_some(Self {
                declared_count,
                lines: Vec::new(),
                inline_text: Some(text),
            })
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillEntry {
    pub vnum: i32,
    pub name: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub skill_type: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<i32>>,
    /// Physical `BASIC` rows in source order.
    pub basic: Vec<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_combo: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<Vec<i32>>,
    pub description: SkillDescription,
}
document!(SkillDocument, SkillEntry);

/// Maps a Skill tag to the row the client reads it as. Any tag beginning
/// with `E`, including `END`, is read as EFFECT. The client never reads
/// `FCOMBO`; it is kept as a source row.
fn skill_row(tag: &str) -> Option<&'static str> {
    Some(match tag.as_bytes()[0] {
        b'V' => "VNUM",
        b'N' => "NAME",
        b'L' => "LEVEL",
        b'E' => "EFFECT",
        b'D' => "DATA",
        b'B' => "BASIC",
        b'Z' => "Z_DESC",
        _ => {
            return ["TYPE", "TARGET", "COST", "CELL", "FCOMBO"]
                .into_iter()
                .find(|exact| *exact == tag);
        }
    })
}

/// How the client converts each value of a Skill row. Values it skips or
/// never reads take -1.
fn skill_value(tag: &str, position: usize) -> Value {
    let zero = matches!(
        (tag, position),
        ("COST", 3..33) | ("EFFECT", 6..9) | ("CELL", 2..93) | ("BASIC", 0)
    );
    Value::Int(if zero { 0 } else { -1 })
}

pub fn decode_skill(text: &str) -> Result<ParsedGtd<SkillDocument>> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut rows = RowReader::new("Skill");
    let mut entries = Vec::new();
    let mut current: Option<SkillEntry> = None;
    let mut next = 0;
    while let Some(line) = lines.get(next) {
        let row = next + 1;
        next += 1;
        let Some((tag, tagged)) = rows.read(row, line, skill_row) else {
            continue;
        };
        match tag {
            "VNUM" => {
                let vnum = rows.scalar(row, tag, &tagged, -1);
                entries.extend(current.replace(SkillEntry {
                    vnum,
                    ..SkillEntry::default()
                }));
            }
            "Z_DESC" => {
                let ([count], defaulted, rest) = leading_values(tagged.rest, [0]);
                if defaulted {
                    rows.malformed(row, tag, &[count]);
                }
                let mut description = SkillDescription {
                    declared_count: count,
                    ..SkillDescription::default()
                };
                if description_count_is_positive(count) {
                    if !rest.is_empty() {
                        rows.warn(row, "text after a positive Skill Z_DESC count is ignored");
                    }
                    if let Some(first) = lines.get(next) {
                        next += 1;
                        description.lines.push(trim(first).to_owned());
                        for _ in 0..100 {
                            let Some(line) = lines.get(next) else {
                                break;
                            };
                            if line.starts_with('#') {
                                break;
                            }
                            next += 1;
                            description.lines.push(trim(line).to_owned());
                        }
                    }
                } else if !rest.is_empty() {
                    description.inline_text = Some(rest.to_owned());
                }
                if let Some(entry) = rows.entry(row, &mut current) {
                    set_skill_description(&mut rows, row, &mut entry.description, description);
                }
            }
            _ => {
                let Some(entry) = rows.entry(row, &mut current) else {
                    continue;
                };
                if tag == "NAME" {
                    set_text(&mut entry.name, tagged.text());
                    continue;
                }
                let values = rows.values(row, tag, &tagged, skill_value);
                let field = match tag {
                    "BASIC" => {
                        entry.basic.push(values);
                        continue;
                    }
                    "TYPE" => &mut entry.skill_type,
                    "COST" => &mut entry.cost,
                    "LEVEL" => &mut entry.level,
                    "EFFECT" => &mut entry.effect,
                    "TARGET" => &mut entry.target,
                    "DATA" => &mut entry.data,
                    "FCOMBO" => &mut entry.final_combo,
                    "CELL" => &mut entry.cell,
                    _ => unreachable!(),
                };
                *field = Some(values);
            }
        }
    }
    entries.extend(current);
    Ok(ParsedGtd {
        document: SkillDocument { entries },
        warnings: rows.finish(),
    })
}

pub fn encode_skill(d: &SkillDocument) -> Result<String> {
    let mut out = String::new();
    for e in &d.entries {
        check_row_text(&e.name, &format!("Skill entry {} name", e.vnum))?;
        push_values(&mut out, "VNUM", &[e.vnum]);
        push_text(&mut out, "NAME", &e.name);
        for (tag, values) in [
            ("TYPE", &e.skill_type),
            ("COST", &e.cost),
            ("LEVEL", &e.level),
            ("EFFECT", &e.effect),
            ("TARGET", &e.target),
            ("DATA", &e.data),
        ] {
            push_optional_values(&mut out, tag, values)
        }
        for b in &e.basic {
            push_values(&mut out, "BASIC", b)
        }
        push_optional_values(&mut out, "FCOMBO", &e.final_combo);
        push_optional_values(&mut out, "CELL", &e.cell);
        push_skill_description(&mut out, e)?;
        out.push_str("#\n")
    }
    Ok(out)
}

/// Stores a `Z_DESC` row's description. The client always takes the new
/// count, but keeps the earlier text when the row has none.
fn set_skill_description(
    rows: &mut RowReader,
    row: usize,
    field: &mut SkillDescription,
    description: SkillDescription,
) {
    if !description.text().is_empty() || field.text().is_empty() {
        *field = description;
        return;
    }
    match field.with_count(description.declared_count) {
        Some(kept) => *field = kept,
        None => {
            rows.warn(
                row,
                format!(
                    "Skill Z_DESC {} row has no text, so the client keeps the earlier description, which one Z_DESC row with that count cannot hold; the earlier description is dropped",
                    description.declared_count
                ),
            );
            *field = description;
        }
    }
}

fn push_skill_description(out: &mut String, e: &SkillEntry) -> Result<()> {
    let count = e.description.declared_count;
    let lines = &e.description.lines;
    let inline_text = e
        .description
        .inline_text
        .as_deref()
        .filter(|text| !text.is_empty());
    if !description_count_is_positive(count) {
        // Blank rows after a non-positive count change nothing in the client.
        if lines.iter().any(|line| !line.is_empty()) {
            return invalid(format!(
                "Skill entry {} has description lines, but its Z_DESC count {count} is not positive; the client reads only inline_text from the Z_DESC row",
                e.vnum
            ));
        }
        match inline_text {
            Some(text) => {
                check_rest_text(text, &format!("Skill entry {} inline_text", e.vnum))?;
                push_text(out, "Z_DESC", &format!("{count}\t{text}"));
            }
            None => push_values(out, "Z_DESC", &[count]),
        }
        return Ok(());
    }

    if inline_text.is_some() {
        return invalid(format!(
            "Skill entry {} has inline_text, but its Z_DESC count {count} is positive; the client reads only the rows after Z_DESC",
            e.vnum
        ));
    }
    if lines.is_empty() {
        return invalid("Skill Z_DESC with a positive count requires a description row");
    }
    if lines.len() > 101 {
        return invalid("Skill Z_DESC cannot contain more than 101 physical rows");
    }
    for (index, line) in lines.iter().enumerate() {
        check_row_text(
            line,
            &format!("Skill entry {} Z_DESC row {}", e.vnum, index + 1),
        )?;
    }
    push_values(out, "Z_DESC", &[count]);
    push_description_rows(out, lines);
    Ok(())
}

/// Stores a text row's text. The client keeps the earlier text when a
/// repeated row has none.
fn set_text(field: &mut String, text: &str) {
    if !text.is_empty() {
        *field = text.to_owned();
    }
}

/// Writes the rows after a positive description count. A later row
/// beginning with `#` would end the client's scan, so it gets a leading
/// space that the client trims.
fn push_description_rows(out: &mut String, lines: &[impl AsRef<str>]) {
    for (index, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        if index > 0 && line.starts_with('#') {
            out.push(' ');
        }
        out.push_str(line);
        out.push('\n');
    }
}

fn push_optional_values(out: &mut String, tag: &str, values: &Option<Vec<i32>>) {
    if let Some(values) = values {
        push_values(out, tag, values);
    }
}

/// Writes grouped values as one row. Every group but the last must be full,
/// so the client reads the same groups back.
fn push_optional_groups(
    out: &mut String,
    tag: &str,
    groups: &Option<Vec<Vec<i32>>>,
    width: usize,
) -> Result<()> {
    let Some(groups) = groups else {
        return Ok(());
    };
    let mut flat = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        let last = index + 1 == groups.len();
        if group.is_empty() || group.len() > width || (!last && group.len() != width) {
            return invalid(format!(
                "{tag} group {} has {} values; groups hold {width} values and only the last may be shorter",
                index + 1,
                group.len()
            ));
        }
        flat.extend(group);
    }
    push_values(out, tag, &flat);
    Ok(())
}

fn chunks(v: Vec<i32>, width: usize) -> Vec<Vec<i32>> {
    v.chunks(width).map(<[i32]>::to_vec).collect()
}
fn exact<T>(v: &[T], expected: usize, name: &str) -> Result<()> {
    if v.len() != expected {
        return invalid(format!(
            "{name} must contain {expected} values, got {}",
            v.len()
        ));
    }
    Ok(())
}
fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(TextError::InvalidGtdDocument {
        message: message.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ignored(tag: &str, text: &str) -> BasicCardIgnoredRow {
        BasicCardIgnoredRow {
            tag: tag.to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn basic_card_slots_follow_the_client_independent_of_desc() {
        let source = concat!(
            "VNUM 1\nICON -1\nNAME n\nDESC 0 0\n",
            "SUBJ0 first\nSUBJ1 second\nSUBJ5 fifth\n",
            "LIST1-1 a\nLIST1-2 b\nLIST3-1 c\nLIST6-1 sixth\nLIST1-3 third\nEND\n~\n",
        );
        let parsed = decode_basic_card(source).unwrap();

        assert!(parsed.warnings.is_empty());
        let entry = &parsed.document.entries[0];
        assert_eq!(entry.description, Some(vec![0, 0]));
        assert_eq!(entry.subject_slots, ["first", "second", "", "", ""]);
        assert_eq!(entry.list_slots[0], ["a", "b"]);
        assert_eq!(entry.list_slots[1], ["", ""]);
        assert_eq!(entry.list_slots[2], ["c", ""]);
        assert_eq!(
            entry.ignored_rows,
            [
                ignored("SUBJ5", "fifth"),
                ignored("LIST6-1", "sixth"),
                ignored("LIST1-3", "third"),
            ]
        );

        let encoded = encode_basic_card(&parsed.document).unwrap();
        assert_eq!(
            encoded,
            concat!(
                "VNUM\t1\nICON\t-1\nNAME\tn\nDESC\t0\t0\n",
                "SUBJ0\tfirst\nSUBJ1\tsecond\n",
                "SUBJ5\tfifth\nLIST6-1\tsixth\nLIST1-3\tthird\n",
                "LIST1-1\ta\nLIST1-2\tb\nLIST3-1\tc\nEND\n",
            )
        );
        assert_eq!(
            decode_basic_card(&encoded).unwrap().document,
            parsed.document
        );
    }

    #[test]
    fn basic_card_keeps_observed_rows() {
        let lists = |indent: &str| {
            (1..=4)
                .map(|slot| {
                    format!("{indent}LIST{slot}-1\tp{slot}\n{indent}LIST{slot}-2\tn{slot}\n")
                })
                .collect::<String>()
        };
        let source = format!(
            concat!(
                "#====\n\tVNUM\t4\n\tICON\t-1\n\tNAME\tzts39e\n\tDESC\t1 1 1 1 2 2\n",
                "\tSUBJ1\ts1\n\tSUBJ2\ts2\n\tSUBJ3\ts3\n\tSUBJ4\ts4\n\tSUBJ5\ts5\n",
                "{}\tLIST5-1\tp5\n\tLIST5-1\tp5\n\tEND\n#====\n\n~\n",
            ),
            lists("\t")
        );
        let parsed = decode_basic_card(&source).unwrap();

        assert!(parsed.warnings.is_empty());
        let entry = &parsed.document.entries[0];
        assert_eq!(entry.icon, Some(-1));
        assert_eq!(entry.description, Some(vec![1, 1, 1, 1, 2, 2]));
        assert_eq!(entry.subject_slots, ["", "s1", "s2", "s3", "s4"]);
        assert_eq!(entry.ignored_rows, [ignored("SUBJ5", "s5")]);
        assert_eq!(entry.list_slots[4], ["p5", ""]);
        assert_eq!(
            encode_basic_card(&parsed.document).unwrap(),
            format!(
                concat!(
                    "VNUM\t4\nICON\t-1\nNAME\tzts39e\nDESC\t1\t1\t1\t1\t2\t2\n",
                    "SUBJ1\ts1\nSUBJ2\ts2\nSUBJ3\ts3\nSUBJ4\ts4\nSUBJ5\ts5\n",
                    "{}LIST5-1\tp5\nEND\n",
                ),
                lists("")
            )
        );
    }

    #[test]
    fn basic_card_entries_need_no_rows_after_vnum() {
        let parsed = decode_basic_card("VNUM 1\nNAME\nVNUM 2\nEND\n").unwrap();

        assert!(parsed.warnings.is_empty());
        let [first, second] = parsed.document.entries.as_slice() else {
            panic!("expected two BCard entries");
        };
        assert_eq!(
            first,
            &BasicCardEntry {
                vnum: 1,
                ..BasicCardEntry::default()
            }
        );
        assert_eq!(second.vnum, 2);

        let encoded = encode_basic_card(&parsed.document).unwrap();
        assert_eq!(encoded, "VNUM\t1\nNAME\nEND\nVNUM\t2\nNAME\nEND\n");
        let reparsed = decode_basic_card(&encoded).unwrap();
        assert!(reparsed.warnings.is_empty());
        assert_eq!(reparsed.document, parsed.document);
    }

    #[test]
    fn basic_card_rows_are_read_by_their_first_character() {
        let source = concat!(
            "SUBJ1 orphan\nVALUE 3 x\nIMAGE 7\nNOTE  spaced  name\nDATA 1 x 3\n",
            "SUBJ01 one\nSXYZ2 two\nL0003-2 negative\nSUBJ\nLIST-1 bad\nFOO bar\n",
            "SUBJ 0\tspaced\n",
        );
        let parsed = decode_basic_card(source).unwrap();

        assert_eq!(
            parsed.warnings,
            [
                warning(1, "BCard row before the first VNUM has no entry"),
                warning(2, "BCard tag VALUE is read as VNUM"),
                warning(2, "malformed BCard VNUM row stored as VNUM 3"),
                warning(3, "BCard tag IMAGE is read as ICON"),
                warning(4, "BCard tag NOTE is read as NAME"),
                warning(5, "BCard tag DATA is read as DESC"),
                warning(5, "malformed BCard DESC row stored as DESC 1 0 3"),
                warning(6, "BCard tag SUBJ01 is read as SUBJ1"),
                warning(7, "BCard tag SXYZ2 is read as SUBJ2"),
                warning(8, "BCard tag L0003-2 is read as LIST3-2"),
                warning(
                    9,
                    "BCard SUBJ row is dropped because its slot index is not plain decimal"
                ),
                warning(
                    10,
                    "BCard LIST-1 row is dropped because its slot index is not plain decimal"
                ),
                warning(11, "unrecognized BCard row"),
                warning(12, "BCard tag SUBJ 0 is read as SUBJ0"),
            ]
        );
        let entry = &parsed.document.entries[0];
        assert_eq!(
            (entry.vnum, entry.icon, entry.name.as_str()),
            (3, Some(7), "spaced  name")
        );
        assert_eq!(entry.description, Some(vec![1, 0, 3]));
        assert_eq!(entry.subject_slots, ["spaced", "one", "two", "", ""]);
        assert_eq!(entry.list_slots[2], ["", "negative"]);
        assert!(entry.ignored_rows.is_empty());
    }

    #[test]
    fn basic_card_repeated_rows_follow_the_client() {
        let source = concat!(
            "VNUM 1\nNAME first\nDESC 1 2 3 4 5 6\n",
            "LIST1-1 keep\nLIST1-2 b\nLIST1-1\nSUBJ2 old\nSUBJ2 new\n",
            "DESC 7 8\nNAME second\nNAME\n",
        );
        let parsed = decode_basic_card(source).unwrap();

        let repeated = "repeated BCard NAME row; the client frees the earlier name, so its result is unreliable";
        assert_eq!(
            parsed.warnings,
            [warning(10, repeated), warning(11, repeated)]
        );
        let entry = &parsed.document.entries[0];
        assert_eq!(entry.name, "second");
        assert_eq!(entry.description, Some(vec![7, 8, 3, 4, 5, 6]));
        assert_eq!(entry.list_slots[0], ["keep", "b"]);
        assert_eq!(entry.subject_slots[2], "new");
    }

    #[test]
    fn basic_card_writer_rejects_rows_the_client_reads_differently() {
        let source = decode_basic_card("VNUM 1\nNAME n\n").unwrap().document;
        let encode = |edit: &dyn Fn(&mut BasicCardEntry)| {
            let mut document = source.clone();
            edit(&mut document.entries[0]);
            encode_basic_card(&document)
        };

        assert!(encode(&|entry| entry.name = "trailing ".to_owned()).is_err());
        assert!(encode(&|entry| entry.subject_slots[0] = " padded".to_owned()).is_err());
        assert!(encode(&|entry| entry.list_slots[4][1] = "split\nrow".to_owned()).is_err());
        for tag in ["SUBJ4", "LIST5-2", "SUBJ 5", "LIST6-1\tx", "NAME", ""] {
            let error = encode(&|entry| entry.ignored_rows = vec![ignored(tag, "x")]);
            assert!(error.is_err(), "{tag}");
        }
        let ignored_rows = vec![ignored("SUBJ5", ""), ignored("LIST0-1", "x")];
        assert_eq!(
            encode(&|entry| entry.ignored_rows = ignored_rows.clone()).unwrap(),
            "VNUM\t1\nNAME\tn\nSUBJ5\nLIST0-1\tx\nEND\n"
        );
    }

    #[test]
    fn basic_card_json_names_the_client_slots() {
        let parsed =
            decode_basic_card("VNUM 1\nNAME n\nDESC 2\nSUBJ0 s\nSUBJ7 t\nLIST2-2 l\n").unwrap();
        assert_eq!(
            serde_json::to_value(&parsed.document).unwrap(),
            serde_json::json!({ "entries": [{
                "vnum": 1, "name": "n", "description": [2],
                "subject_slots": ["s", "", "", "", ""],
                "list_slots": [["", ""], ["", "l"], ["", ""], ["", ""], ["", ""]],
                "ignored_rows": [{ "tag": "SUBJ7", "text": "t" }],
            }]})
        );

        let old = serde_json::json!({ "entries": [{
            "vnum": 1, "icon": -1, "name": "n", "description": [0],
            "subjects": ["s"], "list": [["a", "b"]],
        }]});
        let error = serde_json::from_value::<BasicCardDocument>(old)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("unknown field") && error.contains("`subject_slots`"),
            "{error}"
        );
    }

    #[test]
    fn item_keeps_line_count_independent() {
        let p=decode_item("VNUM 7 10\nNAME zts1e\nINDEX 0 0 0 0 0 0\nTYPE 0 1\nFLAG 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\nDATA 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\nBUFF 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\nLINEDESC 31\nzts2e\nEND\n").unwrap();
        assert_eq!(p.document.entries[0].line_desc_count, 31);
        assert_eq!(p.document.entries[0].description.as_deref(), Some("zts2e"));
        assert!(
            encode_item(&p.document)
                .unwrap()
                .contains("LINEDESC\t31\nzts2e\n")
        )
    }
    #[test]
    fn card_effect_serializes_arbitrary_token_counts() {
        let base = CardDocument {
            kits: vec![vec![String::new(); 5]; 3],
            extra_texts: vec![String::new(); 20],
            entries: vec![CardEntry {
                vnum: 1,
                name: "n".into(),
                group: Some(vec![0; 2]),
                style: Some(vec![0; 5]),
                effect: Some(vec![]),
                icon: None,
                time: Some(vec![0; 2]),
                first_stage: Some(vec![0; 18]),
                second_stage: Some(vec![0; 12]),
                last: Some(vec![0; 2]),
                description: "d".into(),
            }],
        };
        assert!(encode_card(&base).is_ok());
        let mut long = base;
        long.entries[0].effect = Some(vec![1, 2, 3, 4, 5, 6, 7]);
        let native = encode_card(&long).unwrap();
        assert!(native.contains("EFFECT\t1\t2\t3\t4\t5\t6\t7\n"));
        assert_eq!(decode_card(&native).unwrap().document, long)
    }
    #[test]
    fn card_preserves_arbitrary_style_widths() {
        for style in [vec![], vec![-1, 2, 3, 4, 5, 6, i32::MAX]] {
            let source_style = style
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            let source = format!(
                "VNUM 1\nNAME n\nGROUP 0 0\nSTYLE {source_style}\nEFFECT 0 0\nTIME 0 0\n1ST {}\n2ST {}\nLAST 0 0\nDESC d\nEND\n~\n",
                zeros(18),
                zeros(12),
            );
            let parsed = decode_card(&source).unwrap();
            assert!(parsed.warnings.is_empty());
            assert_eq!(parsed.document.entries[0].style, Some(style));
            assert_eq!(
                decode_card(&encode_card(&parsed.document).unwrap())
                    .unwrap()
                    .document,
                parsed.document
            );
        }
    }

    #[test]
    fn item_preserves_arbitrary_flag_widths_and_description_state() {
        for flags in [vec![], (-13..=13).collect::<Vec<_>>()] {
            let source_flags = flags
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            let source = format!(
                "VNUM 7 10\nNAME zts1e\nINDEX {}\nTYPE 0 1\nFLAG {source_flags}\nDATA {}\nBUFF {}\nLINEDESC 23\nzts2e\nEND\n~\n",
                zeros(6),
                zeros(20),
                zeros(25),
            );
            let parsed = decode_item(&source).unwrap();
            let entry = &parsed.document.entries[0];
            assert!(parsed.warnings.is_empty());
            assert_eq!(entry.flags, Some(flags));
            assert_eq!(entry.line_desc_count, 23);
            assert_eq!(entry.description.as_deref(), Some("zts2e"));
            assert_eq!(
                decode_item(&encode_item(&parsed.document).unwrap())
                    .unwrap()
                    .document,
                parsed.document
            );
        }
    }

    #[test]
    fn monster_preserves_arbitrary_widths_and_partial_item_groups() {
        let source = format!(
            concat!(
                "VNUM 1\nNAME zts1e\nLEVEL 1\nRACE\nATTRIB {}\n",
                "HP/MP 0 0\nEXP 0 0\nPREATT {}\nSETTING 1 2 3 4 5 6 7\nETC -9\n",
                "PETINFO 1 2 3 4 5 6\nEFF {}\nZSKILL {}\nWINFO {}\nWEAPON {}\n",
                "AINFO {}\nARMOR {}\nSKILL {}\nPARTNER {}\nBASIC {}\n",
                "CARD {}\nMODE {}\nITEM 2000 9000 1 -1\n~\n"
            ),
            zeros(6),
            zeros(5),
            zeros(3),
            zeros(7),
            zeros(3),
            zeros(7),
            zeros(2),
            zeros(5),
            zeros(15),
            zeros(20),
            zeros(50),
            zeros(20),
            zeros(33),
        );
        let parsed = decode_monster(&source).unwrap();
        let entry = &parsed.document.entries[0];
        assert!(parsed.warnings.is_empty());
        assert_eq!(entry.race, Some(vec![]));
        assert_eq!(entry.settings, Some(vec![1, 2, 3, 4, 5, 6, 7]));
        assert_eq!(entry.etc, Some(vec![-9]));
        assert_eq!(entry.pet_info, Some(vec![1, 2, 3, 4, 5, 6]));
        assert_eq!(entry.mode.as_ref().map(|mode| mode.len()), Some(33));
        assert_eq!(entry.items, Some(vec![vec![2000, 9000, 1], vec![-1]]));
        assert_eq!(
            decode_monster(&encode_monster(&parsed.document).unwrap())
                .unwrap()
                .document,
            parsed.document
        );
    }

    #[test]
    fn skill_preserves_arbitrary_cost_effect_and_cell_widths() {
        let source = format!(
            "VNUM 1\nNAME zts1e\nTYPE {}\nCOST\nLEVEL {}\nEFFECT -5 -4 -3 -2 -1 0 1 2 3 4\nTARGET {}\nDATA {}\n{}FCOMBO {}\nCELL -1 2\nZ_DESC 0\n\n~\n",
            zeros(6),
            zeros(5),
            zeros(5),
            zeros(15),
            (0..5)
                .map(|slot| format!("BASIC {slot} 0 0 0 0 0\n"))
                .collect::<String>(),
            zeros(16),
        );
        let parsed = decode_skill(&source).unwrap();
        let entry = &parsed.document.entries[0];
        assert!(parsed.warnings.is_empty());
        assert_eq!(entry.cost, Some(vec![]));
        assert_eq!(entry.effect, Some(vec![-5, -4, -3, -2, -1, 0, 1, 2, 3, 4]));
        assert_eq!(entry.cell, Some(vec![-1, 2]));
        assert_eq!(
            decode_skill(&encode_skill(&parsed.document).unwrap())
                .unwrap()
                .document,
            parsed.document
        );
    }

    #[test]
    fn act_description_preserves_both_source_tables() {
        let parsed = decode_act_description("Data 7 2 3 4\nend\nA 2 zts1e\n~\n").unwrap();
        assert_eq!(parsed.document.data[0].vnum, 7);
        assert_eq!(parsed.document.data[0].max_ts, 4);
        assert_eq!(parsed.document.titles[0].title, "zts1e");
        assert_eq!(
            encode_act_description(&parsed.document).unwrap(),
            "Data\t7\t2\t3\t4\nend\nA\t2\tzts1e\n~\n"
        );
    }

    #[test]
    fn skill_description_count_is_independent_from_lines() {
        let source = format!(
            "VNUM 1\nNAME zts1e\nTYPE {}\nCOST {}\nLEVEL {}\nEFFECT {}\nTARGET {}\nDATA {}\n{}FCOMBO {}\nCELL {}\nZ_DESC 31\nzts2e\n\n",
            zeros(6),
            zeros(33),
            zeros(5),
            zeros(9),
            zeros(5),
            zeros(15),
            (0..5)
                .map(|slot| format!("BASIC {slot} 0 0 0 0 0\n"))
                .collect::<String>(),
            zeros(16),
            zeros(93),
        );
        let parsed = decode_skill(&source).unwrap();
        assert_eq!(parsed.document.entries[0].description.declared_count, 31);
        assert_eq!(parsed.document.entries[0].description.lines, ["zts2e", ""]);
        assert!(
            encode_skill(&parsed.document)
                .unwrap()
                .contains("Z_DESC\t31\nzts2e\n")
        );
    }

    #[test]
    fn card_entries_end_on_vnum_or_eof_without_end_rows() {
        let source = format!("{}{}~\n", card_record(1), card_record(2));
        let parsed = decode_card(&source).unwrap();

        assert!(parsed.warnings.is_empty());
        assert_eq!(
            parsed
                .document
                .entries
                .iter()
                .map(|entry| entry.vnum)
                .collect::<Vec<_>>(),
            [1, 2]
        );
    }

    #[test]
    fn item_non_positive_descriptions_are_the_rest_of_the_linedesc_row() {
        let source = format!(
            "{} zero  count\n{}",
            item_record(1, 0).trim_end(),
            item_record(2, -7).replace("LINEDESC -7", "LINEDESC\t-7\tnegative\tcount"),
        );
        let parsed = decode_item(&source).unwrap();

        assert!(parsed.warnings.is_empty());
        let [first, second] = parsed.document.entries.as_slice() else {
            panic!("expected two Item entries");
        };
        assert_eq!(first.line_desc_count, 0);
        assert_eq!(first.description, None);
        assert_eq!(first.inline_description.as_deref(), Some("zero  count"));
        assert_eq!(second.line_desc_count, -7);
        assert_eq!(
            second.inline_description.as_deref(),
            Some("negative\tcount")
        );

        let encoded = encode_item(&parsed.document).unwrap();
        assert!(encoded.contains("LINEDESC\t0\tzero  count\nEND\n"));
        assert_eq!(decode_item(&encoded).unwrap().document, parsed.document);
    }

    #[test]
    fn item_non_positive_linedesc_reads_no_following_row() {
        let source = format!("{}NAME override\nEND\n", item_record(1, 0));
        let parsed = decode_item(&source).unwrap();

        assert!(parsed.warnings.is_empty());
        let entry = &parsed.document.entries[0];
        assert_eq!(entry.name, "override");
        assert_eq!(entry.description, None);
        assert_eq!(entry.inline_description, None);
    }

    #[test]
    fn description_counts_use_the_signed_low_word() {
        for (count, positive) in [
            (65536, false),
            (32768, false),
            (65535, false),
            (-65535, true),
        ] {
            let item = decode_item(&format!("{}NAME renamed\nEND\n", item_record(1, count)))
                .unwrap()
                .document;
            let skill = decode_skill(&format!("{}NAME renamed\n#\n", skill_record(1, count)))
                .unwrap()
                .document;
            let (item, skill) = (&item.entries[0], &skill.entries[0]);
            assert_eq!(item.line_desc_count, count);
            assert_eq!(skill.description.declared_count, count);
            if positive {
                assert_eq!(
                    (item.name.as_str(), item.description.as_deref()),
                    ("n1", Some("NAME renamed"))
                );
                assert_eq!(
                    (skill.name.as_str(), skill.description.lines.as_slice()),
                    ("n1", ["NAME renamed".to_owned()].as_slice())
                );
            } else {
                assert_eq!(
                    (item.name.as_str(), item.description.as_deref()),
                    ("renamed", None)
                );
                assert_eq!(
                    (skill.name.as_str(), skill.description.lines.len()),
                    ("renamed", 0)
                );
            }
        }
    }

    #[test]
    fn item_writer_places_descriptions_where_the_client_reads_them() {
        let source = format!(
            "{} Dragon blade\nEND\n{} Very rare\nEND\n",
            item_record(1, 0).trim_end(),
            item_record(2, 0).trim_end(),
        );
        let mut document = decode_item(&source).unwrap().document;
        let encoded = encode_item(&document).unwrap();
        assert!(encoded.contains("LINEDESC\t0\tDragon blade\nEND\nVNUM\t2\t10\n"));
        assert_eq!(decode_item(&encoded).unwrap().document, document);

        document.entries[0].description = Some("Dragon blade".to_owned());
        document.entries[0].inline_description = None;
        let error = encode_item(&document).unwrap_err().to_string();
        assert!(error.contains("inline_description"), "{error}");

        document.entries[0].line_desc_count = 65536;
        assert!(encode_item(&document).is_err());

        document.entries[0].line_desc_count = 1;
        document.entries[0].inline_description = Some("inline".to_owned());
        assert!(encode_item(&document).is_err());

        document.entries[0].inline_description = None;
        document.entries[0].description = Some("first\n second".to_owned());
        assert!(encode_item(&document).is_err());
    }

    #[test]
    fn item_positive_count_without_description_keeps_the_next_item() {
        let mut document = decode_item(&format!("{}first\nEND\n", item_record(1, 1)))
            .unwrap()
            .document;
        document.entries[0].description = None;
        document.entries.push(document.entries[0].clone());
        document.entries[1].vnum = 2;

        let encoded = encode_item(&document).unwrap();
        assert!(encoded.contains("LINEDESC\t1\n\nEND\nVNUM\t2\t10\n"));
        assert_eq!(decode_item(&encoded).unwrap().document, document);
    }

    #[test]
    fn item_description_starting_with_end_reuses_the_previous_append_limit() {
        let source = format!(
            "{}desc one\nEND\n{}END\n{}desc three\nEND\n",
            item_record(1, 1),
            item_record(2, 5),
            item_record(3, 1),
        );
        let parsed = decode_item(&source).unwrap();

        assert_eq!(parsed.document.entries.len(), 2);
        let description = parsed.document.entries[1].description.as_deref().unwrap();
        assert!(description.starts_with("\nVNUM 3 10\nNAME n3\n"));
        assert!(description.ends_with("\nLINEDESC 1\ndesc three"));
        assert_eq!(
            decode_item(&encode_item(&parsed.document).unwrap())
                .unwrap()
                .document,
            parsed.document
        );
    }

    #[test]
    fn item_vnum_rows_always_start_a_new_entry() {
        let source = format!(
            "NAME orphan\nVNUM 5\nNAME five\nLINEDESC 1\nfive desc\nEND\nVNUM 6 10\nLINEDESC 0\nEND\n{}",
            "~\n"
        );
        let parsed = decode_item(&source).unwrap();

        assert_eq!(
            parsed.warnings,
            [
                warning(1, "Item row before the first VNUM has no entry"),
                warning(2, "malformed Item VNUM row stored as VNUM 5 0"),
            ]
        );
        let [five, six] = parsed.document.entries.as_slice() else {
            panic!("expected two Item entries");
        };
        assert_eq!((five.vnum, five.price, five.name.as_str()), (5, 0, "five"));
        assert_eq!(five.description.as_deref(), Some("five desc"));
        assert_eq!((six.vnum, six.price, six.name.as_str()), (6, 10, ""));
        assert_eq!(six.description, None);
        assert_eq!(six.index, None);

        let reparsed = decode_item(&encode_item(&parsed.document).unwrap()).unwrap();
        assert!(reparsed.warnings.is_empty());
        assert_eq!(reparsed.document, parsed.document);
    }

    #[test]
    fn item_signed_labels_are_reported_when_packing_changes_their_count() {
        let flag = |signed: bool| {
            let mut flags = [0; 25];
            flags[ITEM_SIGNED_FLAG] = i32::from(signed);
            let flags = flags.map(|flag| flag.to_string()).join(" ");
            format!("FLAG {flags}\n")
        };
        let (signed, unsigned) = (flag(true), flag(false));
        let labels = |client: usize, packed: usize| {
            format!(
                "Item name has {client} signed-item labels in the client but {packed} after packing, which writes NAME before one FLAG row"
            )
        };
        let source = [
            format!("VNUM 1 0\nNAME one\n{signed}NAME late\n"),
            format!("VNUM 2 0\nNAME two\n{signed}{signed}"),
            format!("VNUM 3 0\nNAME three\n{signed}{unsigned}"),
            format!("VNUM 4 0\nNAME four\n{unsigned}{signed}"),
            format!("VNUM 5 0\n{signed}NAME five\n{signed}"),
            format!("VNUM 6 0\nNAME six\n{signed}NAME\n"),
        ]
        .concat();
        let parsed = decode_item(&source).unwrap();

        assert_eq!(
            parsed.warnings,
            [
                warning(4, labels(0, 1)),
                warning(8, labels(2, 1)),
                warning(12, labels(1, 0)),
            ]
        );
        let names = parsed
            .document
            .entries
            .iter()
            .map(|entry| entry.name.as_str());
        assert!(names.eq(["late", "two", "three", "four", "five", "six"]));
    }

    #[test]
    fn repeated_item_index_rows_are_reported_after_a_type_list_entry() {
        let source = concat!(
            "VNUM 1 0\nINDEX 0 1\nINDEX 0 2\n",
            "VNUM 2 0\nINDEX 4\nINDEX 9\n",
            "VNUM 3 0\nINDEX 10\nINDEX 65540\n",
            "VNUM 4 0\nINDEX -1\nINDEX 7\nINDEX 65536\n",
        );
        let parsed = decode_item(source).unwrap();

        let repeated = "repeated Item INDEX row; the client adds the item to a type list for each INDEX row, but packing writes only the last one";
        assert_eq!(
            parsed.warnings,
            [warning(3, repeated), warning(9, repeated)]
        );
        assert_eq!(parsed.document.entries[3].index, Some(vec![65536]));
    }

    #[test]
    fn repeated_empty_text_rows_keep_the_earlier_text() {
        let card = decode_card("VNUM 5 junk\nNAME five\nDESC a\nDESC\nNAME\n").unwrap();
        let entry = &card.document.entries[0];
        assert_eq!(
            (entry.name.as_str(), entry.description.as_str()),
            ("five", "a")
        );
        let encoded = encode_card(&card.document).unwrap();
        assert!(encoded.contains("VNUM\t5\nNAME\tfive\nDESC\ta\nEND\n"));

        let item = decode_item("VNUM 1 0\nNAME one\nNAME \t \n").unwrap();
        assert_eq!(item.document.entries[0].name, "one");
        let monster = decode_monster("VNUM 6\nNAME six\nNAME\n").unwrap();
        assert_eq!(monster.document.entries[0].name, "six");
        let skill = decode_skill("VNUM 7\nNAME seven\nNAME\nNAME eight\n").unwrap();
        assert_eq!(skill.document.entries[0].name, "eight");
        assert!(item.warnings.is_empty() && monster.warnings.is_empty());
        assert!(skill.warnings.is_empty());
    }

    #[test]
    fn skill_z_desc_without_text_keeps_the_earlier_description() {
        let source = concat!(
            "VNUM 1\nZ_DESC 0 first\nZ_DESC -1\n",
            "VNUM 2\nZ_DESC 2\na\nb\n#\nZ_DESC 3\n\n#\n",
            "VNUM 3\nZ_DESC 1\none\n#\nZ_DESC 0\n",
            "VNUM 4\nZ_DESC 2\na\nb\n#\nZ_DESC 0\n",
            "VNUM 5\nZ_DESC 0  lead\nZ_DESC 1\n\n#\n",
        );
        let parsed = decode_skill(source).unwrap();

        let dropped = |row: usize, count: i32| {
            warning(
                row,
                format!(
                    "Skill Z_DESC {count} row has no text, so the client keeps the earlier description, which one Z_DESC row with that count cannot hold; the earlier description is dropped"
                ),
            )
        };
        assert_eq!(parsed.warnings, [dropped(22, 0), dropped(25, 1)]);
        let descriptions = parsed
            .document
            .entries
            .iter()
            .map(|entry| &entry.description)
            .collect::<Vec<_>>();
        let inline = |declared_count: i32, text: Option<&str>| SkillDescription {
            declared_count,
            lines: Vec::new(),
            inline_text: text.map(str::to_owned),
        };
        assert_eq!(descriptions[0], &inline(-1, Some("first")));
        assert_eq!(descriptions[1].declared_count, 3);
        assert_eq!(descriptions[1].lines, ["a", "b"]);
        assert_eq!(descriptions[2], &inline(0, Some("one")));
        assert_eq!(descriptions[3], &inline(0, None));
        assert_eq!(descriptions[4].lines, [""]);

        let encoded = encode_skill(&parsed.document).unwrap();
        assert!(encoded.contains("Z_DESC\t-1\tfirst\n#\n"));
        assert!(encoded.contains("Z_DESC\t3\na\nb\n#\n"));
        let reparsed = decode_skill(&encoded).unwrap();
        assert!(reparsed.warnings.is_empty());
        assert_eq!(reparsed.document, parsed.document);
    }

    #[test]
    fn skill_text_on_the_z_desc_row_is_inline_text() {
        let parsed = decode_skill("VNUM 1\nNAME n1\nZ_DESC 40000 NAME renamed\n#\n").unwrap();
        let description = &parsed.document.entries[0].description;
        assert_eq!(description.declared_count, 40000);
        assert!(description.lines.is_empty());
        assert_eq!(description.inline_text.as_deref(), Some("NAME renamed"));
        let encoded = encode_skill(&parsed.document).unwrap();
        assert!(encoded.contains("Z_DESC\t40000\tNAME renamed\n#\n"));
        assert_eq!(decode_skill(&encoded).unwrap().document, parsed.document);

        // Older documents stored rows after any positive 32-bit count.
        let old = |declared_count: i32, lines: &[&str]| {
            serde_json::from_value::<SkillDocument>(serde_json::json!({ "entries": [{
                "vnum": 1, "name": "n1", "basic": [],
                "description": { "declared_count": declared_count, "lines": lines },
            }]}))
            .unwrap()
        };
        let error = encode_skill(&old(40000, &["NAME renamed"]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("inline_text"), "{error}");
        assert!(
            encode_skill(&old(40000, &[""]))
                .unwrap()
                .ends_with("Z_DESC\t40000\n#\n")
        );
        assert!(
            encode_skill(&old(4, &["a", "b"]))
                .unwrap()
                .ends_with("Z_DESC\t4\na\nb\n#\n")
        );
    }

    #[test]
    fn description_rows_beginning_with_hash_are_kept() {
        let item = decode_item("VNUM 1 1\nLINEDESC 3\n#first\n  #x\n\t#y\nEND\n").unwrap();
        assert!(item.warnings.is_empty());
        let description = item.document.entries[0].description.as_deref();
        assert_eq!(description, Some("#first\n#x\n#y"));
        let encoded = encode_item(&item.document).unwrap();
        assert!(encoded.ends_with("LINEDESC\t3\n#first\n #x\n #y\nEND\n"));
        assert_eq!(decode_item(&encoded).unwrap().document, item.document);

        let skill = decode_skill("VNUM 1\nZ_DESC 2\nfirst\n  #second\n#\n").unwrap();
        assert!(skill.warnings.is_empty());
        assert_eq!(
            skill.document.entries[0].description.lines,
            ["first", "#second"]
        );
        let encoded = encode_skill(&skill.document).unwrap();
        assert!(encoded.ends_with("Z_DESC\t2\nfirst\n #second\n#\n"));
        assert_eq!(decode_skill(&encoded).unwrap().document, skill.document);
    }

    #[test]
    fn old_item_documents_load_unless_a_description_follows_a_non_positive_count() {
        let entry = |line_desc_count: i32, description: &str| {
            serde_json::json!({
                "vnum": 1, "price": 10, "name": "n",
                "index": [0, 0, 0, 0, 0, 0], "type": [0, 1], "flags": [0],
                "data": [0], "buffs": [[0, 0, 0, 0, 0]],
                "line_desc_count": line_desc_count, "description": description,
            })
        };
        let document = |entry| {
            serde_json::from_value::<ItemDocument>(serde_json::json!({ "entries": [entry] }))
                .unwrap()
        };

        let blank = encode_item(&document(entry(0, ""))).unwrap();
        assert!(blank.ends_with("LINEDESC\t0\nEND\n"));
        let positive = encode_item(&document(entry(3, "a\nb"))).unwrap();
        assert!(positive.ends_with("LINEDESC\t3\na\nb\nEND\n"));
        let error = encode_item(&document(entry(0, "historical row")))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not positive"), "{error}");
    }

    #[test]
    fn card_keeps_physical_text_icon_and_entries_without_optional_rows() {
        let source = concat!(
            "KIT 1 2 kit  text\nZ_ETC 3  lead\n",
            "VNUM 1\nNAME A  B\nEFFECT 0 5 0\nICON 77\nDESC x\tsp  y\nEND\n",
            "VNUM 2\nNAME second\nICON 9\nEFFECT 1 2 3\nEND\n~\n",
        );
        let parsed = decode_card(source).unwrap();

        assert_eq!(
            parsed.warnings,
            [warning(7, "Card tag DESC x is read as DESC")]
        );
        assert_eq!(parsed.document.kits[1][2], "kit  text");
        assert_eq!(parsed.document.extra_texts[3], "lead");
        let [first, second] = parsed.document.entries.as_slice() else {
            panic!("expected two Card entries");
        };
        assert_eq!(first.name, "A  B");
        assert_eq!(first.icon, Some(77));
        assert_eq!(first.description, "sp  y");
        assert_eq!(first.group, None);
        assert_eq!(second.icon, None);
        assert_eq!(second.effect, Some(vec![1, 2, 3]));
        assert_eq!(second.description, "");

        let encoded = encode_card(&parsed.document).unwrap();
        assert!(encoded.contains("EFFECT\t0\t5\t0\nICON\t77\nDESC\tsp  y\nEND\n"));
        let reparsed = decode_card(&encoded).unwrap();
        assert!(reparsed.warnings.is_empty());
        assert_eq!(reparsed.document, parsed.document);
    }

    #[test]
    fn card_vnum_rows_always_start_a_new_entry() {
        let source = format!("NAME orphan\nVNUM 5 junk\nNAME five\n{}", card_record(6));
        let parsed = decode_card(&source).unwrap();

        assert_eq!(
            parsed.warnings,
            [
                warning(1, "Card row before the first VNUM has no entry"),
                warning(2, "malformed Card VNUM row stored as VNUM 5"),
            ]
        );
        let [five, six] = parsed.document.entries.as_slice() else {
            panic!("expected two Card entries");
        };
        assert_eq!(
            (five.vnum, five.name.as_str(), five.description.as_str()),
            (5, "five", "")
        );
        assert_eq!(
            (six.vnum, six.name.as_str(), six.description.as_str()),
            (6, "n6", "d6")
        );
    }

    #[test]
    fn card_rows_are_read_by_their_first_character() {
        let parsed = decode_card("VALUE 3\nNOTE renamed\nGRP 1 2\nEFF 1\n").unwrap();

        assert_eq!(
            parsed.warnings,
            [
                warning(1, "Card tag VALUE is read as VNUM"),
                warning(2, "Card tag NOTE is read as NAME"),
                warning(3, "Card tag GRP is read as GROUP"),
                warning(4, "unrecognized Card row"),
            ]
        );
        let entry = &parsed.document.entries[0];
        assert_eq!((entry.vnum, entry.name.as_str()), (3, "renamed"));
        assert_eq!(entry.group, Some(vec![1, 2]));
        assert_eq!(entry.effect, None);
    }

    #[test]
    fn numeric_values_follow_the_client_conversion() {
        let card = decode_card("VNUM $10\nGROUP 1 x\nTIME -$1F 0x7\nKIT\t1 \t2\tkit\n").unwrap();
        assert_eq!(
            card.warnings,
            [
                warning(2, "malformed Card GROUP row stored as GROUP 1 -1"),
                warning(4, "malformed Card KIT row stored as KIT 0 2"),
            ]
        );
        let entry = &card.document.entries[0];
        assert_eq!(entry.vnum, 16);
        assert_eq!(entry.group, Some(vec![1, -1]));
        assert_eq!(entry.time, Some(vec![-31, 7]));
        assert_eq!(card.document.kits[0][2], "kit");

        let item = decode_item("VNUM 1 $A\nFLAG x y\nLINEDESC $FFFF text\n").unwrap();
        assert_eq!(
            item.warnings,
            [warning(2, "malformed Item FLAG row stored as FLAG -1 0")]
        );
        let entry = &item.document.entries[0];
        assert_eq!(
            (entry.price, entry.flags.as_deref()),
            (10, Some(&[-1, 0][..]))
        );
        assert_eq!(entry.line_desc_count, 65535);
        assert_eq!(entry.inline_description.as_deref(), Some("text"));

        let monster = decode_monster(concat!(
            "VNUM 1\nSETTING a b c d e f\nZSKILL a b c\nWINFO a b c\nAINFO a b\n",
            "ETC 1 x True $1 0.5 false x\nITEM 1 x\n",
        ))
        .unwrap();
        let entry = &monster.document.entries[0];
        assert_eq!(entry.settings, Some(vec![-1, -1, -1, 1, 0, -1]));
        assert_eq!(entry.z_skills, Some(vec![-1, -1, 0]));
        assert_eq!(entry.weapon_info, Some(vec![-1, -1, 0]));
        assert_eq!(entry.armor_info, Some(vec![-1, 0]));
        assert_eq!(entry.etc, Some(vec![1, -1, 1, 0, 1, 0, -1]));
        assert_eq!(entry.items, Some(vec![vec![1, -1]]));
        assert_eq!(monster.warnings.len(), 6);

        let skill = decode_skill(concat!(
            "VNUM 1\nCOST a b c d\nEFFECT a b c d e f g\nCELL a b c\nBASIC x y\n",
            "FCOMBO x\nZ_DESC x\n",
        ))
        .unwrap();
        let entry = &skill.document.entries[0];
        assert_eq!(entry.cost, Some(vec![-1, -1, -1, 0]));
        assert_eq!(entry.effect, Some(vec![-1, -1, -1, -1, -1, -1, 0]));
        assert_eq!(entry.cell, Some(vec![-1, -1, 0]));
        assert_eq!(entry.basic, [vec![0, -1]]);
        assert_eq!(entry.final_combo, Some(vec![-1]));
        assert_eq!(entry.description.declared_count, 0);
        assert_eq!(
            skill.warnings.last(),
            Some(&warning(7, "malformed Skill Z_DESC row stored as Z_DESC 0"))
        );
    }

    #[test]
    fn entity_writers_reject_text_the_client_reads_differently() {
        let mut card = decode_card(&card_record(1)).unwrap().document;
        card.entries[0].name = " padded".to_owned();
        assert!(encode_card(&card).is_err());
        card.entries[0].name = "n".to_owned();
        card.kits[0][0] = "split\nrow".to_owned();
        assert!(encode_card(&card).is_err());

        let mut monster = decode_monster("VNUM 1\nNAME n\n").unwrap().document;
        monster.entries[0].name = "trailing\t".to_owned();
        assert!(encode_monster(&monster).is_err());

        let mut skill = decode_skill(&skill_record(1, 0)).unwrap().document;
        skill.entries[0].description.inline_text = Some("inline ".to_owned());
        assert!(encode_skill(&skill).is_err());
        skill.entries[0].description.inline_text = Some("  inline".to_owned());
        assert!(
            encode_skill(&skill)
                .unwrap()
                .contains("Z_DESC\t0\t  inline\n#\n")
        );
        skill.entries[0].description.declared_count = 2;
        assert!(encode_skill(&skill).is_err());
        skill.entries[0].description.inline_text = None;
        skill.entries[0].description.lines = vec!["one".to_owned(), "two".to_owned()];
        assert!(encode_skill(&skill).is_ok());
        skill.entries[0].description.lines[1] = "two ".to_owned();
        assert!(encode_skill(&skill).is_err());
    }

    #[test]
    fn monster_entries_need_only_the_rows_the_client_reads() {
        let source = concat!(
            "VNUM 1\nNAME first  monster\nATTRIB 3\nPREATT 1 2 3\nZSKILL 0 0 1 2 3\n",
            "VNUM 5 7\nVALUE 6\nSKILL 1 2 3 4\n~\n",
        );
        let parsed = decode_monster(source).unwrap();

        assert_eq!(
            parsed.warnings,
            [
                warning(6, "malformed monster VNUM row stored as VNUM 5"),
                warning(7, "monster tag VALUE is read as VNUM"),
            ]
        );
        let [first, five, six] = parsed.document.entries.as_slice() else {
            panic!("expected three monster entries");
        };
        assert_eq!(first.name, "first  monster");
        assert_eq!(first.attributes, Some(vec![3]));
        assert_eq!(first.z_skills, Some(vec![0, 0, 1, 2, 3]));
        assert_eq!(
            (
                first.effects.as_ref(),
                first.partner.as_ref(),
                first.items.as_ref()
            ),
            (None, None, None)
        );
        assert_eq!(five.vnum, 5);
        assert_eq!(five.skills, None);
        assert_eq!(six.skills, Some(vec![vec![1, 2, 3], vec![4]]));

        let reparsed = decode_monster(&encode_monster(&parsed.document).unwrap()).unwrap();
        assert!(reparsed.warnings.is_empty());
        assert_eq!(reparsed.document, parsed.document);
    }

    #[test]
    fn monster_writer_rejects_groups_the_client_reads_differently() {
        let mut document = decode_monster("VNUM 1\nNAME n\nCARD 1 2 3 4 5 6\n")
            .unwrap()
            .document;
        assert_eq!(
            document.entries[0].cards,
            Some(vec![vec![1, 2, 3, 4, 5], vec![6]])
        );
        document.entries[0].cards = Some(vec![vec![1, 2], vec![3, 4, 5, 6, 7]]);
        assert!(encode_monster(&document).is_err());
    }

    #[test]
    fn skill_basic_rows_keep_their_physical_widths_and_order() {
        let source = concat!(
            "VNUM 1\nNAME n\nBASIC 4 1 2 3 4\nBASIC 0 0 0 0 0 0\nBASIC 7 1 1 1 1\n",
            "BASIC 1 0 0 0 0 0\nBASIC 2 0 0 0 0 0\nBASIC 4 5 6 7 8 9\n",
            "Z_DESC 0\n#\nVNUM 2\nNAME second\n",
        );
        let parsed = decode_skill(source).unwrap();

        assert!(parsed.warnings.is_empty());
        let [first, second] = parsed.document.entries.as_slice() else {
            panic!("expected two Skill entries");
        };
        assert_eq!(
            first.basic,
            [
                vec![4, 1, 2, 3, 4],
                vec![0; 6],
                vec![7, 1, 1, 1, 1],
                vec![1, 0, 0, 0, 0, 0],
                vec![2, 0, 0, 0, 0, 0],
                vec![4, 5, 6, 7, 8, 9]
            ]
        );
        assert_eq!(first.final_combo, None);
        assert!(second.basic.is_empty());

        let encoded = encode_skill(&parsed.document).unwrap();
        assert!(encoded.contains("BASIC\t4\t1\t2\t3\t4\nBASIC\t0\t0\t0\t0\t0\t0\n"));
        assert_eq!(decode_skill(&encoded).unwrap().document, parsed.document);
    }

    #[test]
    fn skill_vnum_and_end_rows_follow_the_client_tags() {
        let source = concat!(
            "BASIC 0 1 1 1 1\nVNUM\nNAME bare\nEFFECT 1 2 3\nEND\n",
            "VNUM 2\nNAME two\nZ_DESC 0  inline  text\n~\n",
        );
        let parsed = decode_skill(source).unwrap();

        assert_eq!(
            parsed.warnings,
            [
                warning(1, "Skill row before the first VNUM has no entry"),
                warning(2, "malformed Skill VNUM row stored as VNUM -1"),
                warning(5, "Skill tag END is read as EFFECT"),
            ]
        );
        let [bare, two] = parsed.document.entries.as_slice() else {
            panic!("expected two Skill entries");
        };
        assert_eq!(bare.vnum, -1);
        assert!(bare.basic.is_empty());
        assert_eq!(bare.effect, Some(vec![]));
        assert!(two.description.lines.is_empty());
        assert_eq!(
            two.description.inline_text.as_deref(),
            Some(" inline  text")
        );

        let reparsed = decode_skill(&encode_skill(&parsed.document).unwrap()).unwrap();
        assert!(reparsed.warnings.is_empty());
        assert_eq!(reparsed.document, parsed.document);
    }

    #[test]
    fn item_positive_description_joins_client_consumed_rows() {
        let source = format!("{} first \n second \nEND\n~\n", item_record(1, 99));
        let parsed = decode_item(&source).unwrap();

        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document.entries.len(), 1);
        assert_eq!(parsed.document.entries[0].line_desc_count, 99);
        assert_eq!(
            parsed.document.entries[0].description.as_deref(),
            Some("first\nsecond")
        );
        assert_eq!(
            decode_item(&encode_item(&parsed.document).unwrap())
                .unwrap()
                .document,
            parsed.document
        );
    }

    #[test]
    fn skill_positive_description_uses_raw_comment_boundary() {
        let source = format!(
            "{}first\n\nEND\n~\nVNUM literal description\n# separator\n{}# final separator\n~\n",
            skill_record(1, 12),
            skill_record(2, 0),
        );
        let parsed = decode_skill(&source).unwrap();

        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document.entries.len(), 2);
        assert_eq!(
            parsed.document.entries[0].description.lines,
            ["first", "", "END", "~", "VNUM literal description"]
        );
        assert!(parsed.document.entries[1].description.lines.is_empty());

        let encoded = encode_skill(&parsed.document).unwrap();
        assert!(encoded.contains("VNUM literal description\n#\nVNUM\t2\n"));
        assert_eq!(decode_skill(&encoded).unwrap().document, parsed.document);
    }

    fn card_record(vnum: i32) -> String {
        format!(
            "VNUM {vnum}\nNAME n{vnum}\nGROUP 0 0\nSTYLE 0 0 0 0 0\nEFFECT 0 0\nTIME 0 0\n1ST {}\n2ST {}\nLAST 0 0\nDESC d{vnum}\n",
            zeros(18),
            zeros(12),
        )
    }

    fn item_record(vnum: i32, line_desc_count: i32) -> String {
        format!(
            "VNUM {vnum} 10\nNAME n{vnum}\nINDEX {}\nTYPE 0 1\nFLAG {}\nDATA {}\nBUFF {}\nLINEDESC {line_desc_count}\n",
            zeros(6),
            zeros(25),
            zeros(20),
            zeros(25),
        )
    }

    fn skill_record(vnum: i32, declared_count: i32) -> String {
        format!(
            "VNUM {vnum}\nNAME n{vnum}\nTYPE {}\nCOST {}\nLEVEL {}\nEFFECT {}\nTARGET {}\nDATA {}\n{}FCOMBO {}\nCELL {}\nZ_DESC {declared_count}\n",
            zeros(6),
            zeros(33),
            zeros(5),
            zeros(9),
            zeros(5),
            zeros(15),
            (0..5)
                .map(|slot| format!("BASIC {slot} 0 0 0 0 0\n"))
                .collect::<String>(),
            zeros(16),
            zeros(93),
        )
    }

    fn zeros(count: usize) -> String {
        std::iter::repeat_n("0", count)
            .collect::<Vec<_>>()
            .join(" ")
    }
}
