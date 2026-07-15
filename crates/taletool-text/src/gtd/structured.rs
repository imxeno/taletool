//! SAdapters for the less regular GTD text records.

use serde::{Deserialize, Serialize};

use super::{ParsedGtd, fields, is_ignored_line, push_text, push_values, values, warning};
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
        push_text(&mut out, "%", &e.vnum.to_string());
        push_text(&mut out, "t", &e.title);
        for s in &e.states {
            push_text(&mut out, "s", &s.vnum.to_string());
            for c in &s.commands {
                let (tag, text) = match c {
                    NpcTalkCommand::C(t) => ("c", t),
                    NpcTalkCommand::B(t) => ("b", t),
                    NpcTalkCommand::F(t) => ("f", t),
                };
                clean_text(text, "npc command")?;
                push_text(&mut out, tag, text);
            }
        }
    }
    Ok(out)
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
        if is_ignored_line(line) {
            continue;
        }
        let f = fields(line);
        if f.first() == Some(&"DATA") {
            match (entries.last_mut(), values(&f[1..])) {
                (Some(e), Some(v)) => e.data_rows.push(v),
                _ => warnings.push(warning(index + 1, "invalid map DATA row")),
            }
        } else if f.len() == 5 {
            match values(&f[..4]) {
                Some(v) => entries.push(MapIdEntry {
                    min_map_vnum: v[0],
                    max_map_vnum: v[1],
                    map_point_vnum: v[2],
                    point_kind: v[3],
                    name: f[4].into(),
                    data_rows: Vec::new(),
                }),
                None => warnings.push(warning(index + 1, "invalid map id row")),
            }
        } else {
            warnings.push(warning(index + 1, "invalid map id row"))
        }
    }
    Ok(ParsedGtd {
        document: MapIdDocument { entries },
        warnings,
    })
}
pub fn encode_map_id(doc: &MapIdDocument) -> Result<String> {
    let mut out = String::new();
    for e in &doc.entries {
        clean_text(&e.name, "map name")?;
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            e.min_map_vnum, e.max_map_vnum, e.map_point_vnum, e.point_kind, e.name
        ));
        for d in &e.data_rows {
            push_values(&mut out, "DATA", d)
        }
        out.push('\n')
    }
    Ok(out)
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
    let mut sections = Vec::new();
    let mut warnings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim() == "~" || is_ignored_line(line) || line.trim() == "E" {
            continue;
        }
        let f = fields(line);
        match f.first().copied() {
            Some("S") if f.len() == 2 => match f[1].parse() {
                Ok(vnum) => sections.push(MapPointSection {
                    vnum,
                    points: Vec::new(),
                }),
                _ => warnings.push(warning(index + 1, "invalid map section")),
            },
            Some("D") if f.len() == 5 => match (values(&f[1..4]), sections.last_mut()) {
                (Some(v), Some(s)) => s.points.push(MapPoint {
                    kind: v[0],
                    x: v[1],
                    y: v[2],
                    name: f[4].into(),
                }),
                _ => warnings.push(warning(index + 1, "invalid map point")),
            },
            _ => warnings.push(warning(index + 1, "invalid map point row")),
        }
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
            clean_text(&p.name, "map point name")?;
            out.push_str(&format!("D\t{}\t{}\t{}\t{}\n", p.kind, p.x, p.y, p.name))
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
    pub weight: i32,
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
        if let Some([vnum]) = ints(line, "VNUM") {
            if let Some(e) = cur.take() {
                entries.push(e)
            }
            cur = Some(FishEntry {
                vnum,
                level: [0; 2],
                declared_map_count: 0,
                maps: Vec::new(),
                declared_item_count: 0,
                items: Vec::new(),
                declared_basic_count: 0,
                basics: Vec::new(),
            });
            map_index = None
        } else if let Some(e) = cur.as_mut() {
            if let Some(v) = ints(line, "LEVEL") {
                e.level = v
            } else if let Some([v]) = ints(line, "MAPT") {
                e.declared_map_count = v
            } else if let Some([slot, map_vnum]) = ints(line, "MAP") {
                e.maps.push(FishMap {
                    slot,
                    map_vnum,
                    declared_position_count: 0,
                    positions: Vec::new(),
                });
                map_index = Some(e.maps.len() - 1)
            } else if let Some([slot, count]) = ints(line, "POST") {
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
            } else if let Some([v]) = ints(line, "ITEMT") {
                e.declared_item_count = v
            } else if let Some(v) = ints::<3>(line, "ITEM") {
                e.items.push(FishItem {
                    slot: v[0],
                    vnum: v[1],
                    weight: v[2],
                })
            } else if let Some([v]) = ints(line, "BASICT") {
                e.declared_basic_count = v
            } else if let Some(v) = ints::<3>(line, "BASIC") {
                e.basics.push(FishItem {
                    slot: v[0],
                    vnum: v[1],
                    weight: v[2],
                })
            } else {
                warnings.push(warning(row, "invalid fish row"))
            }
        } else {
            warnings.push(warning(row, "fish row before VNUM"))
        }
    }
    if let Some(e) = cur {
        entries.push(e)
    }
    Ok(ParsedGtd {
        document: FishDocument { entries },
        warnings,
    })
}
pub fn encode_fish(doc: &FishDocument) -> Result<String> {
    let mut out = String::new();
    for e in &doc.entries {
        if e.declared_map_count < 0
            || e.declared_item_count < 0
            || e.declared_basic_count < 0
            || e.maps.iter().any(|map| map.declared_position_count < 0)
        {
            return Err(invalid("fish declared counts must be non-negative"));
        }
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
            push_values(&mut out, "ITEM", &[i.slot, i.vnum, i.weight])
        }
        push_values(&mut out, "BASICT", &[e.declared_basic_count]);
        for i in &e.basics {
            push_values(&mut out, "BASIC", &[i.slot, i.vnum, i.weight])
        }
    }
    out.push_str("~\n");
    Ok(out)
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
