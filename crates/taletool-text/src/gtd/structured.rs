//! SAdapters for the less regular GTD text records.

use serde::{Deserialize, Serialize};

use super::row_tokens::{client_int, leading_values, split_token, tokens, trim};
use super::{
    GtdWarning, ParsedGtd, fields, is_ignored_line, push_text, push_values, values, warning,
};
use crate::{Result, TextError};

fn invalid(message: impl Into<String>) -> TextError {
    TextError::InvalidGtdDocument {
        message: message.into(),
    }
}

fn ints<const N: usize>(line: &str, tag: &str) -> Option<[i32; N]> {
    let f = fields(line.split_once("//").map_or(line, |(data, _)| data));
    if !f.first()?.eq_ignore_ascii_case(tag) || f.len() != N + 1 {
        return None;
    }
    values(&f[1..])?.try_into().ok()
}

fn text_after<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
    let line = line.trim();
    let rest = line.strip_prefix(tag)?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim_start())
}

fn clean_text(value: &str, field: &str) -> Result<()> {
    if value.contains(['\r', '\n']) {
        return Err(invalid(format!("{field} contains a line break")));
    }
    Ok(())
}

fn push_bare_values(out: &mut String, values: &[i32]) {
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            out.push('\t');
        }
        out.push_str(&value.to_string());
    }
    out.push('\n');
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcTalkDocument {
    pub entries: Vec<NpcTalkEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcTalkEntry {
    pub vnum: i32,
    pub title: String,
    pub states: Vec<NpcTalkState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcTalkState {
    pub vnum: i32,
    pub commands: Vec<NpcTalkCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "op",
    content = "text",
    rename_all = "lowercase",
    deny_unknown_fields
)]
pub enum NpcTalkCommand {
    C(String),
    B(String),
    F(String),
}

pub fn decode_npc_talk(text: &str) -> Result<ParsedGtd<NpcTalkDocument>> {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut entry: Option<NpcTalkEntry> = None;
    let mut state: Option<NpcTalkState> = None;
    for (index, raw) in text.lines().enumerate().skip(1) {
        let row = index + 1;
        let line = raw.trim_end_matches('\r');
        if line.trim() == "~" || is_ignored_line(line) {
            continue;
        }
        if let Some(rest) = line.trim_start().strip_prefix('%') {
            match rest.trim().parse() {
                Ok(vnum) => {
                    finish_npc_state(&mut entry, &mut state);
                    if let Some(old) = entry.take() {
                        entries.push(old);
                    }
                    entry = Some(NpcTalkEntry {
                        vnum,
                        title: String::new(),
                        states: Vec::new(),
                    })
                }
                Err(_) => warnings.push(warning(row, "invalid npc talk vnum")),
            }
        } else if let Some(title) = text_after(line, "t") {
            if let Some(e) = entry.as_mut() {
                e.title = title.to_owned();
            } else {
                warnings.push(warning(row, "title outside entry"));
            }
        } else if let Some(value) = text_after(line, "s") {
            finish_npc_state(&mut entry, &mut state);
            match value.parse() {
                Ok(vnum) if entry.is_some() => {
                    state = Some(NpcTalkState {
                        vnum,
                        commands: Vec::new(),
                    })
                }
                _ => warnings.push(warning(row, "invalid state")),
            }
        } else {
            let parsed = [
                ('c', NpcTalkCommand::C as fn(String) -> _),
                ('b', NpcTalkCommand::B as fn(String) -> _),
                ('f', NpcTalkCommand::F as fn(String) -> _),
            ]
            .into_iter()
            .find_map(|(tag, ctor)| {
                line.strip_prefix(tag)
                    .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
                    .map(|r| ctor(r.trim_start().to_owned()))
            });
            match (state.as_mut(), parsed) {
                (Some(s), Some(c)) => s.commands.push(c),
                _ => warnings.push(warning(row, "unrecognized npc talk row")),
            }
        }
    }
    finish_npc_state(&mut entry, &mut state);
    if let Some(e) = entry {
        entries.push(e);
    }
    entries.retain(|e| {
        if e.title.is_empty() {
            warnings.push(warning(0, format!("npc talk {} has no title", e.vnum)));
            false
        } else {
            true
        }
    });
    Ok(ParsedGtd {
        document: NpcTalkDocument { entries },
        warnings,
    })
}

fn finish_npc_state(entry: &mut Option<NpcTalkEntry>, state: &mut Option<NpcTalkState>) {
    if let (Some(e), Some(s)) = (entry.as_mut(), state.take()) {
        e.states.push(s);
    }
}

pub fn encode_npc_talk(doc: &NpcTalkDocument) -> Result<String> {
    let mut out = "# generated npc talk\n".to_owned();
    for e in &doc.entries {
        clean_text(&e.title, "npc title")?;
        if e.title.is_empty() {
            return Err(invalid("npc title is required"));
        }
        push_npc_talk_row(&mut out, "%", &e.vnum.to_string());
        push_npc_talk_row(&mut out, "t", &e.title);
        for s in &e.states {
            push_npc_talk_row(&mut out, "s", &s.vnum.to_string());
            for c in &s.commands {
                let (tag, text) = match c {
                    NpcTalkCommand::C(t) => ("c", t),
                    NpcTalkCommand::B(t) => ("b", t),
                    NpcTalkCommand::F(t) => ("f", t),
                };
                clean_text(text, "npc command")?;
                push_npc_talk_row(&mut out, tag, text);
            }
        }
    }
    Ok(out)
}

/// The client splits npctalk rows once at the first space, never at a tab.
fn push_npc_talk_row(out: &mut String, tag: &str, text: &str) {
    out.push_str(tag);
    if !text.is_empty() {
        out.push(' ');
        out.push_str(text);
    }
    out.push('\n');
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestDocument {
    pub entries: Vec<QuestEntry>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestEntry {
    pub vnum: Vec<i32>,
    pub level: Vec<i32>,
    pub title: String,
    pub description: String,
    pub talk: [i32; 4],
    pub target: [i32; 3],
    pub data: Vec<[i32; 4]>,
    pub prize: [i32; 4],
    pub link: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<Vec<i32>>,
}

pub fn decode_quest(text: &str) -> Result<ParsedGtd<QuestDocument>> {
    let blocks = tagged_blocks(text, "BEGIN");
    let mut entries = Vec::new();
    let mut warnings = blocks.1;
    for (row, lines) in blocks.0 {
        let mut vnum = None;
        let mut level = None;
        let mut title = None;
        let mut description = None;
        let mut talk = None;
        let mut target = None;
        let mut data = Vec::new();
        let mut prize = None;
        let mut link = None;
        let mut objective = None;
        for (r, l) in lines {
            let numeric = l.split_once("//").map_or(l, |(data, _)| data);
            let numeric_fields = fields(numeric);
            if numeric_fields
                .first()
                .is_some_and(|tag| tag.eq_ignore_ascii_case("VNUM"))
            {
                match values(&numeric_fields[1..]) {
                    Some(values) => vnum = Some(values),
                    None => warnings.push(warning(r, "invalid quest VNUM row")),
                }
            } else if fields(l).first() == Some(&"LEVEL") {
                level = values(&fields(l)[1..]);
            } else if let Some(v) = text_after(l, "TITLE") {
                title = Some(v.into())
            } else if let Some(v) = text_after(l, "DESC") {
                description = Some(v.into())
            } else if let Some(v) = ints(l, "TALK") {
                talk = Some(v)
            } else if let Some(v) = ints(l, "TARGET") {
                target = Some(v)
            } else if let Some(v) = ints(l, "DATA") {
                data.push(v)
            } else if let Some(v) = ints(l, "PRIZE") {
                prize = Some(v)
            } else if let Some(v) = ints::<1>(l, "LINK") {
                link = Some(v[0])
            } else if fields(l).first() == Some(&"O") {
                objective = values(&fields(l)[1..]);
            } else {
                warnings.push(warning(r, "unrecognized quest row"));
            }
        }
        match (vnum, level, title, description, talk, target, prize, link) {
            (
                Some(vnum),
                Some(level),
                Some(title),
                Some(description),
                Some(talk),
                Some(target),
                Some(prize),
                Some(link),
            ) => entries.push(QuestEntry {
                vnum,
                level,
                title,
                description,
                talk,
                target,
                data,
                prize,
                link,
                objective,
            }),
            _ => warnings.push(warning(row, "incomplete quest block")),
        }
    }
    Ok(ParsedGtd {
        document: QuestDocument { entries },
        warnings,
    })
}

pub fn encode_quest(doc: &QuestDocument) -> Result<String> {
    let mut out = String::new();
    for e in &doc.entries {
        clean_text(&e.title, "quest title")?;
        clean_text(&e.description, "quest description")?;
        out.push_str("BEGIN\n");
        push_values(&mut out, "VNUM", &e.vnum);
        push_values(&mut out, "LEVEL", &e.level);
        push_text(&mut out, "TITLE", &e.title);
        push_text(&mut out, "DESC", &e.description);
        push_values(&mut out, "TALK", &e.talk);
        push_values(&mut out, "TARGET", &e.target);
        for d in &e.data {
            push_values(&mut out, "DATA", d)
        }
        push_values(&mut out, "PRIZE", &e.prize);
        push_values(&mut out, "LINK", &[e.link]);
        if let Some(o) = &e.objective {
            push_values(&mut out, "O", o)
        }
        out.push_str("END\n\n");
    }
    Ok(out)
}

type TaggedBlocks<'a> = Vec<(usize, Vec<(usize, &'a str)>)>;

fn tagged_blocks<'a>(text: &'a str, start: &str) -> (TaggedBlocks<'a>, Vec<super::GtdWarning>) {
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    let mut current: Option<(usize, Vec<_>)> = None;
    for (index, raw) in text.lines().enumerate() {
        let row = index + 1;
        let line = raw.trim();
        if line.eq_ignore_ascii_case("END") || line == "~" || is_ignored_line(line) {
            continue;
        }
        if line.eq_ignore_ascii_case(start) {
            if let Some(block) = current.replace((row, Vec::new())) {
                blocks.push(block)
            }
        } else if let Some((_, rows)) = current.as_mut() {
            rows.push((row, line))
        } else {
            warnings.push(warning(row, "row outside block"))
        }
    }
    if let Some(block) = current {
        blocks.push(block)
    }
    (blocks, warnings)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestPrizeDocument {
    pub entries: Vec<QuestPrizeEntry>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestPrizeEntry {
    pub vnum: [i32; 2],
    pub data: [i32; 5],
}
pub fn decode_quest_prize(text: &str) -> Result<ParsedGtd<QuestPrizeDocument>> {
    let (blocks, mut warnings) = tagged_blocks(text, "BEGIN");
    let mut entries = Vec::new();
    for (row, lines) in blocks {
        let mut v = None;
        let mut d = None;
        for (r, l) in lines {
            if let Some(x) = ints(l, "VNUM") {
                v = Some(x)
            } else if let Some(x) = ints(l, "DATA") {
                d = Some(x)
            } else {
                warnings.push(warning(r, "unrecognized quest prize row"))
            }
        }
        match (v, d) {
            (Some(vnum), Some(data)) => entries.push(QuestPrizeEntry { vnum, data }),
            _ => warnings.push(warning(row, "incomplete quest prize block")),
        }
    }
    Ok(ParsedGtd {
        document: QuestPrizeDocument { entries },
        warnings,
    })
}
pub fn encode_quest_prize(doc: &QuestPrizeDocument) -> Result<String> {
    let mut out = String::new();
    for e in &doc.entries {
        out.push_str("BEGIN\n");
        push_values(&mut out, "VNUM", &e.vnum);
        push_values(&mut out, "DATA", &e.data);
        out.push_str("END\n\n")
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TutorialDocument {
    pub entries: Vec<TutorialScript>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TutorialScript {
    pub vnum: i32,
    pub commands: Vec<TutorialCommand>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TutorialCommand {
    pub step: i32,
    pub text: String,
}
pub fn decode_tutorial(text: &str) -> Result<ParsedGtd<TutorialDocument>> {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut cur = None;
    for (index, raw) in text.lines().enumerate() {
        let row = index + 1;
        let line = raw.trim();
        if line != "~" && is_ignored_line(line) {
            continue;
        }
        let f = fields(line);
        if f.first()
            .and_then(|token| token.get(..3))
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("END"))
        {
            continue;
        }
        if f.first().is_some_and(|v| v.eq_ignore_ascii_case("script")) {
            if let Some(s) = cur.take() {
                entries.push(s)
            }
            match f.get(1).and_then(|v| v.parse().ok()) {
                Some(vnum) => {
                    cur = Some(TutorialScript {
                        vnum,
                        commands: Vec::new(),
                    })
                }
                None => warnings.push(warning(row, "invalid tutorial script")),
            }
        } else if let Some(script) = cur.as_mut() {
            let (step, text) = line
                .split_once(char::is_whitespace)
                .map_or((line, ""), |(step, text)| (step, text.trim_start()));
            let step = step.parse().unwrap_or_else(|_| {
                warnings.push(warning(row, "invalid tutorial step normalized to -1"));
                -1
            });
            script.commands.push(TutorialCommand {
                step,
                text: text.to_owned(),
            });
        } else {
            warnings.push(warning(row, "tutorial command outside script"))
        }
    }
    if let Some(s) = cur {
        entries.push(s)
    }
    Ok(ParsedGtd {
        document: TutorialDocument { entries },
        warnings,
    })
}
pub fn encode_tutorial(doc: &TutorialDocument) -> Result<String> {
    let mut out = String::new();
    for s in &doc.entries {
        push_text(&mut out, "script", &s.vnum.to_string());
        for c in &s.commands {
            clean_text(&c.text, "tutorial command")?;
            out.push_str(&c.step.to_string());
            out.push('\t');
            out.push_str(&c.text);
            out.push('\n')
        }
        out.push_str("end\n")
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShopTypeDocument {
    pub entries: Vec<ShopTypeEntry>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShopTypeEntry {
    pub vnum: i32,
    pub types: Vec<i32>,
}
pub fn decode_shop_type(text: &str) -> Result<ParsedGtd<ShopTypeDocument>> {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim() == "~" {
            entries.push(ShopTypeEntry {
                vnum: -1,
                types: Vec::new(),
            });
            continue;
        }
        if is_ignored_line(line) {
            continue;
        }
        let f = fields(line);
        match (
            f.first().and_then(|v| v.parse().ok()),
            values(f.get(1..).unwrap_or_default()),
        ) {
            (Some(vnum), Some(types)) if types.len() <= 6 => {
                entries.push(ShopTypeEntry { vnum, types })
            }
            _ => warnings.push(warning(index + 1, "invalid shop type row")),
        }
    }
    Ok(ParsedGtd {
        document: ShopTypeDocument { entries },
        warnings,
    })
}
pub fn encode_shop_type(doc: &ShopTypeDocument) -> Result<String> {
    let mut out = String::new();
    for e in &doc.entries {
        if e.types.len() > 6 {
            return Err(invalid("shop type has more than six types"));
        }
        let mut v = vec![e.vnum];
        v.extend_from_slice(&e.types);
        push_bare_values(&mut out, &v)
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapIdDocument {
    pub entries: Vec<MapIdEntry>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapIdEntry {
    pub min_map_vnum: i32,
    pub max_map_vnum: i32,
    pub map_point_vnum: i32,
    pub point_kind: i32,
    pub name: String,
    pub data_rows: Vec<Vec<i32>>,
}
pub fn decode_map_id(text: &str) -> Result<ParsedGtd<MapIdDocument>> {
    let mut entries: Vec<MapIdEntry> = Vec::new();
    let mut warnings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        let (tag, rest) = split_token(line);
        if tag.is_empty() || tag.starts_with('#') {
            continue;
        }
        // Every other row whose first character is not `D` starts an entry.
        if !tag.starts_with('D') {
            let ([min_map_vnum, max_map_vnum, map_point_vnum, point_kind], normalized, name) =
                leading_values(line);
            if normalized {
                warnings.push(warning(row, "invalid map id value normalized to -1"));
            }
            entries.push(MapIdEntry {
                min_map_vnum,
                max_map_vnum,
                map_point_vnum,
                point_kind,
                name: name.into(),
                data_rows: Vec::new(),
            });
            continue;
        }
        let Some(entry) = entries.last_mut() else {
            warnings.push(warning(row, "map DATA row before the first map entry"));
            continue;
        };
        if tag.contains(' ') {
            warnings.push(warning(row, "text in the map DATA tag dropped"));
        }
        // The client reads only the first value; the rest stay as source.
        let mut normalized = false;
        let values = tokens(rest)
            .into_iter()
            .map(|token| {
                client_int(token).unwrap_or_else(|| {
                    normalized = true;
                    -1
                })
            })
            .collect();
        if normalized {
            warnings.push(warning(row, "invalid map DATA value normalized to -1"));
        }
        entry.data_rows.push(values);
    }
    Ok(ParsedGtd {
        document: MapIdDocument { entries },
        warnings,
    })
}
pub fn encode_map_id(doc: &MapIdDocument) -> Result<String> {
    let mut out = String::new();
    for e in &doc.entries {
        remainder_text(&e.name, "map name")?;
        out.push_str(&format!(
            "{}\t{}\t{}\t{}",
            e.min_map_vnum, e.max_map_vnum, e.map_point_vnum, e.point_kind
        ));
        push_remainder_text(&mut out, &e.name);
        for d in &e.data_rows {
            push_values(&mut out, "DATA", d)
        }
        out.push('\n')
    }
    Ok(out)
}

/// The client keeps everything after a row's numeric fields as text, but trims
/// the row first, so text cannot end in whitespace.
fn remainder_text(value: &str, field: &str) -> Result<()> {
    clean_text(value, field)?;
    if value.ends_with(|c: char| c <= ' ') {
        return Err(invalid(format!("{field} ends with whitespace")));
    }
    Ok(())
}

/// Ends a row after its numeric fields, separating any text with a tab.
fn push_remainder_text(out: &mut String, text: &str) {
    if !text.is_empty() {
        out.push('\t');
        out.push_str(text);
    }
    out.push('\n');
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapPointDocument {
    pub sections: Vec<MapPointSection>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapPointSection {
    pub vnum: i32,
    pub points: Vec<MapPoint>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapPoint {
    pub kind: i32,
    pub x: i32,
    pub y: i32,
    pub name: String,
}
pub fn decode_map_point(text: &str) -> Result<ParsedGtd<MapPointDocument>> {
    let mut sections: Vec<MapPointSection> = Vec::new();
    let mut warnings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        if line.trim() == "~" || is_ignored_line(line) || line.trim() == "E" {
            continue;
        }
        let (tag, rest) = split_token(line);
        if !tag.starts_with(['S', 'D']) {
            warnings.push(warning(row, "invalid map point row"));
            continue;
        }
        if tag.contains(' ') {
            warnings.push(warning(row, "text in the map point tag dropped"));
        }
        if tag.starts_with('S') {
            // The client parses the whole remainder as the section number.
            let vnum = client_int(rest).unwrap_or_else(|| {
                warnings.push(warning(row, "invalid map section normalized to -1"));
                -1
            });
            sections.push(MapPointSection {
                vnum,
                points: Vec::new(),
            });
            continue;
        }
        let Some(section) = sections.last_mut() else {
            warnings.push(warning(row, "map point before the first section"));
            continue;
        };
        let ([kind, x, y], normalized, name) = leading_values(rest);
        if normalized {
            warnings.push(warning(row, "invalid map point value normalized to -1"));
        }
        section.points.push(MapPoint {
            kind,
            x,
            y,
            name: name.into(),
        });
    }
    Ok(ParsedGtd {
        document: MapPointDocument { sections },
        warnings,
    })
}
pub fn encode_map_point(doc: &MapPointDocument) -> Result<String> {
    let mut out = String::new();
    for s in &doc.sections {
        push_values(&mut out, "S", &[s.vnum]);
        for p in &s.points {
            remainder_text(&p.name, "map point name")?;
            out.push_str(&format!("D\t{}\t{}\t{}", p.kind, p.x, p.y));
            push_remainder_text(&mut out, &p.name);
        }
    }
    out.push_str("E\n");
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestNpcDocument {
    pub rows: Vec<QuestNpcRow>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuestNpcRow {
    Mode0 {
        npc_vnum: i32,
        values: [i32; 4],
    },
    Mode1 {
        npc_vnum: i32,
        quest_vnum: i32,
        unknown: i32,
        level: i32,
    },
}
pub fn decode_quest_npc(text: &str) -> Result<ParsedGtd<QuestNpcDocument>> {
    let mut rows = Vec::new();
    let mut warnings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim() == "~" || is_ignored_line(line) {
            continue;
        }
        let f = fields(line);
        match values(&f) {
            Some(v) if v.len() == 6 && v[1] == 0 => rows.push(QuestNpcRow::Mode0 {
                npc_vnum: v[0],
                values: v[2..].try_into().unwrap(),
            }),
            Some(v) if v.len() == 5 && v[1] == 1 => rows.push(QuestNpcRow::Mode1 {
                npc_vnum: v[0],
                quest_vnum: v[2],
                unknown: v[3],
                level: v[4],
            }),
            _ => warnings.push(warning(index + 1, "invalid quest npc row")),
        }
    }
    Ok(ParsedGtd {
        document: QuestNpcDocument { rows },
        warnings,
    })
}
pub fn encode_quest_npc(doc: &QuestNpcDocument) -> Result<String> {
    let mut out = String::new();
    for r in &doc.rows {
        match r {
            QuestNpcRow::Mode0 { npc_vnum, values } => push_bare_values(
                &mut out,
                &[*npc_vnum, 0, values[0], values[1], values[2], values[3]],
            ),
            QuestNpcRow::Mode1 {
                npc_vnum,
                quest_vnum,
                unknown,
                level,
            } => push_bare_values(&mut out, &[*npc_vnum, 1, *quest_vnum, *unknown, *level]),
        }
    }
    out.push_str("~\n");
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamDocument {
    pub entries: Vec<TeamEntry>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamEntry {
    pub vnum: [i32; 2],
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub target: [i32; 4],
    pub buff: [i32; 4],
}
type PendingTeam = (
    usize,
    Option<[i32; 2]>,
    Option<String>,
    Option<String>,
    Option<[i32; 4]>,
    Option<[i32; 4]>,
);
pub fn decode_team(text: &str) -> Result<ParsedGtd<TeamDocument>> {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut cur: Option<PendingTeam> = None;
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        if line.trim() == "~" || is_ignored_line(line) {
            continue;
        }
        if let Some(v) = ints(line, "VNUM") {
            if let Some(c) = cur.take() {
                finish_team(c, &mut entries, &mut warnings)
            }
            cur = Some((row, Some(v), None, None, None, None))
        } else if let Some(c) = cur.as_mut() {
            if let Some(v) = text_after(line, "TITLE") {
                c.2 = Some(v.into())
            } else if let Some(v) = text_after(line, "DESC") {
                c.3 = Some(v.into())
            } else if let Some(v) = ints(line, "TARGET") {
                c.4 = Some(v)
            } else if let Some(v) = ints(line, "BUFF") {
                c.5 = Some(v)
            } else {
                warnings.push(warning(row, "invalid team row"))
            }
        } else {
            warnings.push(warning(row, "team row before VNUM"))
        }
    }
    if let Some(c) = cur {
        finish_team(c, &mut entries, &mut warnings)
    }
    Ok(ParsedGtd {
        document: TeamDocument { entries },
        warnings,
    })
}
fn finish_team(
    c: PendingTeam,
    entries: &mut Vec<TeamEntry>,
    warnings: &mut Vec<super::GtdWarning>,
) {
    match c {
        (_, Some(vnum), Some(title), description, Some(target), Some(buff)) => {
            entries.push(TeamEntry {
                vnum,
                title,
                description,
                target,
                buff,
            })
        }
        (row, ..) => warnings.push(warning(row, "incomplete team entry")),
    }
}
pub fn encode_team(doc: &TeamDocument) -> Result<String> {
    let mut out = String::new();
    for e in &doc.entries {
        clean_text(&e.title, "team title")?;
        push_values(&mut out, "VNUM", &e.vnum);
        push_text(&mut out, "TITLE", &e.title);
        if let Some(d) = &e.description {
            clean_text(d, "team description")?;
            push_text(&mut out, "DESC", d)
        }
        push_values(&mut out, "TARGET", &e.target);
        push_values(&mut out, "BUFF", &e.buff);
        out.push('\n')
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FishDocument {
    pub entries: Vec<FishEntry>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FishEntry {
    pub vnum: i32,
    pub level: [i32; 2],
    pub declared_map_count: i32,
    pub maps: Vec<FishMap>,
    pub declared_item_count: i32,
    pub items: Vec<FishItem>,
    pub declared_basic_count: i32,
    pub basics: Vec<FishItem>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FishMap {
    pub slot: i32,
    pub map_vnum: i32,
    pub declared_position_count: i32,
    pub positions: Vec<FishPosition>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FishPosition {
    pub map_slot: i32,
    pub slot: i32,
    pub x: i32,
    pub y: i32,
    pub direction: i32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FishItem {
    pub slot: i32,
    pub vnum: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<i32>,
}
pub fn decode_fish(text: &str) -> Result<ParsedGtd<FishDocument>> {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut cur: Option<FishEntry> = None;
    let mut map_index = None;
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        if line.trim() == "~" || is_ignored_line(line) {
            continue;
        }
        let (tag, rest) = split_token(line);
        let tag = trim(tag).to_ascii_uppercase();
        if tag == "VNUM" {
            // The client starts a fish at every VNUM row, whatever follows it.
            let ([vnum], unread) = fish_values(rest, row, &mut warnings);
            unread_fish_text(unread, row, &mut warnings);
            let next = FishEntry {
                vnum,
                level: [0; 2],
                declared_map_count: 0,
                maps: Vec::new(),
                declared_item_count: 0,
                items: Vec::new(),
                declared_basic_count: 0,
                basics: Vec::new(),
            };
            if let Some(e) = cur.replace(next) {
                entries.push(e)
            }
            map_index = None;
            continue;
        }
        let Some(e) = cur.as_mut() else {
            warnings.push(warning(row, "fish row before VNUM"));
            continue;
        };
        let unread = match tag.as_str() {
            "LEVEL" => {
                let (level, unread) = fish_values(rest, row, &mut warnings);
                e.level = level;
                unread
            }
            "MAPT" => {
                let ([count], unread) = fish_values(rest, row, &mut warnings);
                e.declared_map_count = count;
                unread
            }
            "MAP" => {
                let ([slot, map_vnum], unread) = fish_values(rest, row, &mut warnings);
                e.maps.push(FishMap {
                    slot,
                    map_vnum,
                    declared_position_count: 0,
                    positions: Vec::new(),
                });
                map_index = Some(e.maps.len() - 1);
                unread
            }
            "ITEMT" => {
                let ([count], unread) = fish_values(rest, row, &mut warnings);
                e.declared_item_count = count;
                unread
            }
            "ITEM" => {
                let ([slot, vnum], unread) = fish_values(rest, row, &mut warnings);
                let (token, after) = split_token(unread);
                let weight = client_int(token);
                e.items.push(FishItem { slot, vnum, weight });
                if weight.is_some() { after } else { unread }
            }
            // The client never reads POST, POS, BASICT or BASIC rows.
            _ => {
                if let Some([slot, count]) = ints(line, "POST") {
                    map_index = e.maps.iter().rposition(|m| m.slot == slot);
                    if let Some(i) = map_index {
                        e.maps[i].declared_position_count = count
                    } else {
                        warnings.push(warning(row, "POST without MAP"))
                    }
                } else if let Some(v) = ints::<5>(line, "POS") {
                    if let Some(i) = map_index {
                        e.maps[i].positions.push(FishPosition {
                            map_slot: v[0],
                            slot: v[1],
                            x: v[2],
                            y: v[3],
                            direction: v[4],
                        })
                    } else {
                        warnings.push(warning(row, "POS without MAP"))
                    }
                } else if let Some([v]) = ints(line, "BASICT") {
                    e.declared_basic_count = v
                } else if let Some([slot, vnum, weight]) = ints(line, "BASIC") {
                    e.basics.push(FishItem {
                        slot,
                        vnum,
                        weight: Some(weight),
                    })
                } else if let Some([slot, vnum]) = ints(line, "BASIC") {
                    e.basics.push(FishItem {
                        slot,
                        vnum,
                        weight: None,
                    })
                } else {
                    warnings.push(warning(row, "invalid fish row"))
                }
                continue;
            }
        };
        unread_fish_text(unread, row, &mut warnings);
    }
    if let Some(e) = cur {
        entries.push(e)
    }
    Ok(ParsedGtd {
        document: FishDocument { entries },
        warnings,
    })
}

/// Reads the `N` values the client takes from a fish row, reporting values
/// that fall back to -1, and returns the unread remainder.
fn fish_values<'a, const N: usize>(
    rest: &'a str,
    row: usize,
    warnings: &mut Vec<GtdWarning>,
) -> ([i32; N], &'a str) {
    let (values, normalized, unread) = leading_values(rest);
    if normalized {
        warnings.push(warning(row, "invalid fish value normalized to -1"));
    }
    (values, unread)
}

/// Reports tokens the client ignores after a fish row's values. A trailing
/// `//` comment is not reported.
fn unread_fish_text(unread: &str, row: usize, warnings: &mut Vec<GtdWarning>) {
    let (token, _) = split_token(unread);
    if !token.is_empty() && !token.starts_with("//") {
        warnings.push(warning(row, "extra fish values dropped"));
    }
}

pub fn encode_fish(doc: &FishDocument) -> Result<String> {
    let mut out = String::new();
    for e in &doc.entries {
        push_values(&mut out, "VNUM", &[e.vnum]);
        push_values(&mut out, "LEVEL", &e.level);
        push_values(&mut out, "MAPT", &[e.declared_map_count]);
        for m in &e.maps {
            push_values(&mut out, "MAP", &[m.slot, m.map_vnum]);
            push_values(&mut out, "POST", &[m.slot, m.declared_position_count]);
            for p in &m.positions {
                push_values(
                    &mut out,
                    "POS",
                    &[p.map_slot, p.slot, p.x, p.y, p.direction],
                )
            }
        }
        push_values(&mut out, "ITEMT", &[e.declared_item_count]);
        for i in &e.items {
            push_fish_item(&mut out, "ITEM", i)
        }
        push_values(&mut out, "BASICT", &[e.declared_basic_count]);
        for i in &e.basics {
            push_fish_item(&mut out, "BASIC", i)
        }
    }
    out.push_str("~\n");
    Ok(out)
}

fn push_fish_item(out: &mut String, tag: &str, item: &FishItem) {
    match item.weight {
        Some(weight) => push_values(out, tag, &[item.slot, item.vnum, weight]),
        None => push_values(out, tag, &[item.slot, item.vnum]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn item_like_count_mismatches_survive_fish() {
        let src = "VNUM 0\nLEVEL 1 50\nMAPT 3\nMAP 0 1\nPOST 0 2\nPOS 0 0 10 20 3\nITEMT 9\nITEM 4 10421 1000\nBASICT 0\n~\n";
        let p = decode_fish(src).unwrap();
        assert_eq!(p.document.entries[0].declared_map_count, 3);
        assert_eq!(p.document.entries[0].maps[0].declared_position_count, 2);
        let again = decode_fish(&encode_fish(&p.document).unwrap()).unwrap();
        assert_eq!(again.document, p.document)
    }

    #[test]
    fn fish_rows_keep_the_values_the_client_reads() {
        let src = "VNUM 1 // first\nLEVEL 5 6 7\nMAPT\nMAP 0 5 9\nITEM 0 2100\nITEM 1 2200 300\n";
        let parsed = decode_fish(src).unwrap();
        let rows: Vec<_> = parsed.warnings.iter().map(|w| w.row).collect();
        assert_eq!(rows, [2, 3, 4]);
        let fish = &parsed.document.entries[0];
        assert_eq!(fish.level, [5, 6]);
        assert_eq!(fish.declared_map_count, -1);
        assert_eq!((fish.maps[0].slot, fish.maps[0].map_vnum), (0, 5));
        assert_eq!(
            fish.items,
            [
                FishItem {
                    slot: 0,
                    vnum: 2100,
                    weight: None,
                },
                FishItem {
                    slot: 1,
                    vnum: 2200,
                    weight: Some(300),
                },
            ]
        );

        let native = encode_fish(&parsed.document).unwrap();
        assert!(native.contains("MAPT\t-1\n"));
        assert!(native.contains("ITEM\t0\t2100\nITEM\t1\t2200\t300\n"));
        let again = decode_fish(&native).unwrap();
        assert!(again.warnings.is_empty());
        assert_eq!(again.document, parsed.document);
    }

    #[test]
    fn fish_vnum_rows_always_start_an_entry() {
        let src = concat!(
            "VNUM 1\nLEVEL 1 10\nMAP 0 5\nITEM 1 2200 100\n",
            "VNUM 2 99\nLEVEL 2 20\nMAP 0 1\nITEM 0 2300 100\n",
            "VNUM fish\nLEVEL 3 30\n",
            "VNUM 4\t99\nLEVEL 4 40\n",
        );
        let parsed = decode_fish(src).unwrap();
        let rows: Vec<_> = parsed.warnings.iter().map(|w| w.row).collect();
        assert_eq!(rows, [5, 9, 11]);
        let entries = &parsed.document.entries;
        let summary: Vec<_> = entries.iter().map(|e| (e.vnum, e.level)).collect();
        // The client reads `VNUM 4<TAB>99` as one unknown tag and ignores it.
        assert_eq!(summary, [(1, [1, 10]), (2, [2, 20]), (-1, [4, 40])]);
        assert_eq!(entries[0].items.len(), 1);
        assert_eq!(entries[1].maps[0].map_vnum, 1);
    }

    #[test]
    fn fish_rows_before_the_first_vnum_are_reported() {
        let parsed = decode_fish("# comment\nLEVEL 1 2\nBASIC 0 1\nVNUM 1\n~\n").unwrap();
        let rows: Vec<_> = parsed.warnings.iter().map(|w| w.row).collect();
        assert_eq!(rows, [2, 3]);
        assert_eq!(parsed.document.entries[0].level, [0, 0]);
    }

    #[test]
    fn fish_basic_rows_stay_source_rows_with_an_optional_weight() {
        let src = "VNUM 1\nBASICT 2\nBASIC 0 9198\nBASIC 1 9208 500\nPOST 0 3\nBASIC x\n";
        let parsed = decode_fish(src).unwrap();
        let rows: Vec<_> = parsed.warnings.iter().map(|w| w.row).collect();
        assert_eq!(rows, [5, 6]);
        let fish = &parsed.document.entries[0];
        assert_eq!(fish.basics[0].weight, None);
        assert_eq!(fish.basics[1].weight, Some(500));
        assert_eq!(
            decode_fish(&encode_fish(&parsed.document).unwrap())
                .unwrap()
                .document,
            parsed.document
        );
    }

    #[test]
    fn fish_item_weight_is_an_optional_json_field() {
        let item = FishItem {
            slot: 0,
            vnum: 2100,
            weight: None,
        };
        let json = serde_json::to_value(&item).unwrap();
        assert!(json.get("weight").is_none());
        assert_eq!(serde_json::from_value::<FishItem>(json).unwrap(), item);
        let weighted: FishItem =
            serde_json::from_str(r#"{"slot":1,"vnum":2200,"weight":300}"#).unwrap();
        assert_eq!(weighted.weight, Some(300));
    }
    #[test]
    fn npc_commands_keep_order() {
        let src = "# header\n% 1\nt name\ns 0\nc hello\nf 1\nb branch data\n";
        let p = decode_npc_talk(src).unwrap();
        assert!(matches!(
            p.document.entries[0].states[0].commands[1],
            NpcTalkCommand::F(_)
        ));
        assert!(
            encode_npc_talk(&p.document)
                .unwrap()
                .starts_with("# generated")
        )
    }

    #[test]
    fn npc_talk_rows_separate_the_tag_with_one_space() {
        let src = "# header\n% 42\nt name\ns 7\nc hello world\nf 1\nb branch data\nc\n";
        let encoded = encode_npc_talk(&decode_npc_talk(src).unwrap().document).unwrap();
        assert_eq!(
            encoded,
            "# generated npc talk\n% 42\nt name\ns 7\nc hello world\nf 1\nb branch data\nc\n"
        );
    }

    #[test]
    fn invalid_percent_row_does_not_close_the_active_client_state() {
        let source = concat!(
            "# header\n% 1\nt title\ns 0\nc before\n",
            "% c malformed source text\nc after\nb branch\n",
            "% 2\nt next\ns 0\nc done\n",
        );
        let parsed = decode_npc_talk(source).unwrap();
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(parsed.document.entries.len(), 2);
        assert_eq!(
            parsed.document.entries[0].states[0].commands,
            [
                NpcTalkCommand::C("before".into()),
                NpcTalkCommand::C("after".into()),
                NpcTalkCommand::B("branch".into()),
            ]
        );
    }
    #[test]
    fn map_point_has_one_final_e() {
        let d = MapPointDocument {
            sections: vec![
                MapPointSection {
                    vnum: 1,
                    points: vec![],
                },
                MapPointSection {
                    vnum: 2,
                    points: vec![],
                },
            ],
        };
        let s = encode_map_point(&d).unwrap();
        assert_eq!(s.lines().filter(|l| *l == "E").count(), 1)
    }

    fn map_id_entry(name: &str, data_rows: Vec<Vec<i32>>) -> MapIdEntry {
        MapIdEntry {
            min_map_vnum: 20,
            max_map_vnum: 30,
            map_point_vnum: 6,
            point_kind: 2,
            name: name.into(),
            data_rows,
        }
    }

    #[test]
    fn map_id_names_keep_the_row_remainder() {
        let src = "1\t10\t5\t1\tFirst\nDATA 7\n20 30 6 2 Two Words\nDATA 9\n40 50 7 3\nDATA 11\n";
        let parsed = decode_map_id(src).unwrap();
        assert!(parsed.warnings.is_empty());
        let names: Vec<_> = parsed.document.entries.iter().map(|e| &e.name).collect();
        assert_eq!(names, ["First", "Two Words", ""]);
        assert_eq!(parsed.document.entries[2].data_rows, [[11]]);

        let native = encode_map_id(&parsed.document).unwrap();
        assert!(native.contains("20\t30\t6\t2\tTwo Words\n"));
        assert!(native.contains("40\t50\t7\t3\nDATA\t11\n"));
        assert_eq!(decode_map_id(&native).unwrap().document, parsed.document);
    }

    #[test]
    fn map_id_rows_after_an_invalid_header_stay_with_it() {
        let src = "1 10 5 1 First\nDATA 7\nmap 30 6 2 Bad\nDATA 9\n~\nDATA 1\n";
        let parsed = decode_map_id(src).unwrap();
        let rows: Vec<_> = parsed.warnings.iter().map(|w| w.row).collect();
        assert_eq!(rows, [3, 5]);
        let entries = &parsed.document.entries;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].data_rows, [[7]]);
        assert_eq!(
            (
                entries[1].min_map_vnum,
                &*entries[1].name,
                &entries[1].data_rows
            ),
            (-1, "Bad", &vec![vec![9]])
        );
        assert_eq!(entries[2], {
            let mut entry = map_id_entry("", vec![vec![1]]);
            (entry.min_map_vnum, entry.max_map_vnum) = (-1, -1);
            (entry.map_point_vnum, entry.point_kind) = (-1, -1);
            entry
        });
    }

    #[test]
    fn map_id_data_rows_are_rows_starting_with_d() {
        let src = "DATA 3\n1 1 1 4 zts1e\nD 5\nDESC 6 7\ndata 8\n";
        let parsed = decode_map_id(src).unwrap();
        assert_eq!(parsed.warnings[0].row, 1);
        let entries = &parsed.document.entries;
        assert_eq!(entries[0].data_rows, [vec![5], vec![6, 7]]);
        assert_eq!(entries[1].min_map_vnum, -1);
        assert_eq!(entries[1].max_map_vnum, 8);
    }

    #[test]
    fn map_id_json_names_with_spaces_or_no_text_read_back() {
        let document = MapIdDocument {
            entries: vec![
                map_id_entry("Two Words", vec![vec![1]]),
                map_id_entry("", vec![]),
                map_id_entry("  indented\tname", vec![vec![], vec![2, 3]]),
            ],
        };
        let parsed = decode_map_id(&encode_map_id(&document).unwrap()).unwrap();
        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document, document);

        for name in ["trailing ", "tab\t", " "] {
            let document = MapIdDocument {
                entries: vec![map_id_entry(name, vec![])],
            };
            assert!(encode_map_id(&document).is_err(), "{name:?}");
        }
    }

    #[test]
    fn map_point_text_keeps_the_row_remainder() {
        let src = "S 1\nD 2 30 40 Two Words\nD\t3\t50\t60\nE\n";
        let parsed = decode_map_point(src).unwrap();
        assert!(parsed.warnings.is_empty());
        let points = &parsed.document.sections[0].points;
        assert_eq!(
            points[0],
            MapPoint {
                kind: 2,
                x: 30,
                y: 40,
                name: "Two Words".into(),
            }
        );
        assert_eq!(points[1].name, "");

        let native = encode_map_point(&parsed.document).unwrap();
        assert!(native.contains("D\t3\t50\t60\n"));
        assert_eq!(decode_map_point(&native).unwrap().document, parsed.document);
        let mut document = parsed.document;
        document.sections[0].points[0].name = "name ".into();
        assert!(encode_map_point(&document).is_err());
    }

    #[test]
    fn map_point_section_reads_its_whole_remainder() {
        let src = "D 1 2 3 Early\nS\t1\nD 1 2 3 One\nS\t2 // comment\nD 4 5 6 Four\nS  3\n";
        let parsed = decode_map_point(src).unwrap();
        let rows: Vec<_> = parsed.warnings.iter().map(|w| w.row).collect();
        assert_eq!(rows, [1, 4]);
        let sections = &parsed.document.sections;
        let vnums: Vec<_> = sections.iter().map(|s| s.vnum).collect();
        assert_eq!(vnums, [1, -1, 3]);
        assert_eq!(sections[0].points.len(), 1);
        assert_eq!(sections[1].points[0].name, "Four");
        assert_eq!(
            decode_map_point(&encode_map_point(&parsed.document).unwrap())
                .unwrap()
                .document,
            parsed.document
        );
    }
    #[test]
    fn quest_repeated_data_round_trips() {
        let src = "BEGIN\nVNUM 1 2 3 4 5 6\nLEVEL 1 99\nTITLE a\nDESC b\nTALK 1 2 3 4\nTARGET 1 2 3\nDATA 1 2 3 4\nDATA 5 6 7 8\nPRIZE 1 2 3 4\nLINK 2\nEND\n";
        let p = decode_quest(src).unwrap();
        assert_eq!(p.document.entries[0].data.len(), 2);
        assert_eq!(
            decode_quest(&encode_quest(&p.document).unwrap())
                .unwrap()
                .document,
            p.document
        )
    }

    #[test]
    fn quest_blocks_end_at_the_next_begin_or_eof() {
        let src = concat!(
            "BEGIN\nVNUM 1 2 3 4 5 6\nLEVEL 1 99\nTITLE first\nEND\n",
            "DESC after-end\nTALK 1 2 3 4\nTARGET 1 2 3\nPRIZE 1 2 3 4\nLINK 2\n",
            "BEGIN\nVNUM 7 8 9 10 11 12\nLEVEL 2 98\nTITLE second\n",
            "DESC eof\nTALK 5 6 7 8\nTARGET 4 5 6\nPRIZE 5 6 7 8\nLINK 3\n",
        );
        let parsed = decode_quest(src).unwrap();
        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document.entries.len(), 2);
        assert_eq!(parsed.document.entries[0].description, "after-end");
        assert_eq!(parsed.document.entries[1].description, "eof");
    }

    #[test]
    fn quest_prize_blocks_ignore_end_and_finalize_at_begin_or_eof() {
        let src = concat!(
            "BEGIN\nVNUM 1 2\nEND\nDATA 3 4 5 6 7\n",
            "BEGIN\nVNUM 8 9\nDATA 10 11 12 13 14\n",
        );
        let parsed = decode_quest_prize(src).unwrap();
        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document.entries.len(), 2);
        assert_eq!(parsed.document.entries[0].data, [3, 4, 5, 6, 7]);
        assert_eq!(parsed.document.entries[1].data, [10, 11, 12, 13, 14]);
    }

    #[test]
    fn quest_preserves_arbitrary_vnum_widths_and_repeated_data() {
        for (vnum, level) in [
            (vec![], vec![]),
            (vec![1, -2, 3, 4, 5, 6, 7, i32::MAX], vec![-1, 2, 3, 4, 5]),
        ] {
            let source_vnum = vnum
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            let source_level = level
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            let src = format!(
                concat!(
                    "BEGIN\nVNUM {source_vnum}\nLEVEL {source_level}\nTITLE a\nDESC b\n",
                    "TALK 1 2 3 4\nTARGET 1 2 3\nDATA 1 2 3 4\nDATA 5 6 7 8\n",
                    "PRIZE -1 -1 -1 -1\nLINK 2\nEND\n~\n"
                ),
                source_vnum = source_vnum,
                source_level = source_level,
            );
            let parsed = decode_quest(&src).unwrap();
            assert!(parsed.warnings.is_empty());
            assert_eq!(parsed.document.entries[0].vnum, vnum);
            assert_eq!(parsed.document.entries[0].level, level);
            assert_eq!(parsed.document.entries[0].data.len(), 2);
            assert_eq!(
                decode_quest(&encode_quest(&parsed.document).unwrap())
                    .unwrap()
                    .document,
                parsed.document
            );
        }
    }

    #[test]
    fn tutorial_uses_one_decorative_end_per_script_without_a_magic_tilde() {
        let document = TutorialDocument {
            entries: vec![
                TutorialScript {
                    vnum: 1,
                    commands: vec![],
                },
                TutorialScript {
                    vnum: 2,
                    commands: vec![],
                },
            ],
        };
        let native = encode_tutorial(&document).unwrap();
        assert_eq!(native.lines().filter(|line| *line == "end").count(), 2);
        assert!(native.ends_with("end\n"));
        assert!(!native.lines().any(|line| line == "~"));
    }

    #[test]
    fn tutorial_normalizes_a_client_consumed_invalid_step_to_minus_one() {
        let parsed = decode_tutorial("script 1\n3초 기다리기\nend\n").unwrap();
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(
            parsed.document.entries[0].commands[0],
            TutorialCommand {
                step: -1,
                text: "기다리기".into(),
            }
        );
        assert_eq!(
            decode_tutorial(&encode_tutorial(&parsed.document).unwrap())
                .unwrap()
                .document,
            parsed.document
        );
    }

    #[test]
    fn tutorial_normalizes_current_tilde_as_a_semantic_dummy_command() {
        let parsed = decode_tutorial("script 1\n1 hello\nend\n~\n").unwrap();
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(
            parsed.document.entries[0].commands,
            [
                TutorialCommand {
                    step: 1,
                    text: "hello".into(),
                },
                TutorialCommand {
                    step: -1,
                    text: String::new(),
                },
            ]
        );
        let native = encode_tutorial(&parsed.document).unwrap();
        assert!(!native.lines().any(|line| line == "~"));
        assert!(native.lines().any(|line| line == "-1\t"));
        assert_eq!(decode_tutorial(&native).unwrap().document, parsed.document);
    }

    #[test]
    fn tutorial_accepts_client_command_tokens_without_text() {
        let parsed = decode_tutorial("script 1\n7\ninvalid\nend\n").unwrap();
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(
            parsed.document.entries[0].commands,
            [
                TutorialCommand {
                    step: 7,
                    text: String::new(),
                },
                TutorialCommand {
                    step: -1,
                    text: String::new(),
                },
            ]
        );
    }

    #[test]
    fn tutorial_ignores_any_command_token_with_the_end_prefix() {
        let parsed =
            decode_tutorial("script 1\n1 before\nENDING ignored\neNdMarker ignored\n2 after\n")
                .unwrap();
        assert!(parsed.warnings.is_empty());
        assert_eq!(
            parsed.document.entries[0].commands,
            [
                TutorialCommand {
                    step: 1,
                    text: "before".into(),
                },
                TutorialCommand {
                    step: 2,
                    text: "after".into(),
                },
            ]
        );
    }

    #[test]
    fn tutorial_old_source_does_not_gain_a_dummy_command() {
        let parsed = decode_tutorial("script 1\n1 hello\nend\n").unwrap();
        let native = encode_tutorial(&parsed.document).unwrap();
        let again = decode_tutorial(&native).unwrap();
        assert_eq!(again.document, parsed.document);
        assert_eq!(again.document.entries[0].commands.len(), 1);
        assert!(!native.lines().any(|line| line == "~"));
    }

    #[test]
    fn shop_type_normalizes_current_tilde_as_a_minus_one_entry() {
        let parsed = decode_shop_type("1 2 3\n~\n").unwrap();
        assert_eq!(
            parsed.document.entries[1],
            ShopTypeEntry {
                vnum: -1,
                types: Vec::new(),
            }
        );
        let native = encode_shop_type(&parsed.document).unwrap();
        assert!(!native.lines().any(|line| line == "~"));
        assert_eq!(decode_shop_type(&native).unwrap().document, parsed.document);
    }

    #[test]
    fn shop_type_old_source_does_not_gain_a_minus_one_entry() {
        let parsed = decode_shop_type("1 2 3\n").unwrap();
        let native = encode_shop_type(&parsed.document).unwrap();
        let again = decode_shop_type(&native).unwrap();
        assert_eq!(again.document, parsed.document);
        assert_eq!(again.document.entries.len(), 1);
        assert!(!native.lines().any(|line| line == "~"));
    }

    #[test]
    fn team_accepts_inline_comments_and_absent_description() {
        let parsed =
            decode_team("VNUM 1 2\nTITLE zts1e\nTARGET 1 2 3 4 // comment\nBUFF 5 6 7 8\n")
                .unwrap();
        assert_eq!(parsed.document.entries[0].description, None);
        assert_eq!(parsed.document.entries[0].target, [1, 2, 3, 4]);
    }

    #[test]
    fn quest_npc_keeps_both_source_row_shapes() {
        let parsed = decode_quest_npc("331 0 2 1 19 1\n1062 1 5000 0 80\n~\n").unwrap();
        assert!(matches!(parsed.document.rows[0], QuestNpcRow::Mode0 { .. }));
        assert!(matches!(parsed.document.rows[1], QuestNpcRow::Mode1 { .. }));
        assert!(encode_quest_npc(&parsed.document).unwrap().ends_with("~\n"));
    }
}
