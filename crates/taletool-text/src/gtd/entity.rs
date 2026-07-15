//! Models for the entity tables in `NSgtdData.NOS`.

use serde::{Deserialize, Serialize};

use super::{
    ParsedGtd, fields, is_ignored_line, parse_i32, push_text, push_values, values, warning,
};
use crate::{Result, TextError};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasicCardEntry {
    pub vnum: i32,
    pub icon: i32,
    pub name: String,
    pub description: Vec<i32>,
    pub subjects: Vec<String>,
    pub list: Vec<Vec<String>>,
}
document!(BasicCardDocument, BasicCardEntry);

const BASIC_CARD_LIST_VALUES_PER_SLOT: usize = 2;

fn validate_basic_card_text(value: &str, field: &str) -> Result<()> {
    if value.contains(['\r', '\n']) {
        return invalid(format!("BCard {field} contains a line break"));
    }
    Ok(())
}

fn validate_basic_card_entry(entry: &BasicCardEntry) -> Result<()> {
    let slot_count = entry.description.len();
    if entry.subjects.len() != slot_count {
        return invalid(format!(
            "BCard entry {} has {} DESC values but {} subjects",
            entry.vnum,
            slot_count,
            entry.subjects.len()
        ));
    }
    if entry.list.len() != slot_count {
        return invalid(format!(
            "BCard entry {} has {} DESC values but {} LIST groups",
            entry.vnum,
            slot_count,
            entry.list.len()
        ));
    }

    validate_basic_card_text(&entry.name, "name")?;
    for (index, subject) in entry.subjects.iter().enumerate() {
        validate_basic_card_text(subject, &format!("SUBJ{}", index + 1))?;
    }
    for (slot_index, values) in entry.list.iter().enumerate() {
        if values.len() != BASIC_CARD_LIST_VALUES_PER_SLOT {
            return invalid(format!(
                "BCard entry {} LIST group {} has {} values; expected {BASIC_CARD_LIST_VALUES_PER_SLOT}",
                entry.vnum,
                slot_index + 1,
                values.len()
            ));
        }
        for (value_index, value) in values.iter().enumerate() {
            validate_basic_card_text(
                value,
                &format!("LIST{}-{}", slot_index + 1, value_index + 1),
            )?;
        }
    }

    Ok(())
}

#[derive(Default)]
struct BasicCardBuilder {
    vnum: Option<i32>,
    icon: Option<i32>,
    name: Option<String>,
    description: Option<Vec<i32>>,
    subjects: std::collections::BTreeMap<usize, String>,
    list: std::collections::BTreeMap<(usize, usize), String>,
}
impl BasicCardBuilder {
    fn new() -> Self {
        Self::default()
    }
    fn finish(mut self) -> Option<BasicCardEntry> {
        let description = self.description?;
        let slot_count = description.len();
        let entry = BasicCardEntry {
            vnum: self.vnum?,
            icon: self.icon?,
            name: self.name?,
            description,
            subjects: (0..slot_count)
                .map(|index| self.subjects.remove(&index).unwrap_or_default())
                .collect(),
            list: (0..slot_count)
                .map(|slot| {
                    (0..BASIC_CARD_LIST_VALUES_PER_SLOT)
                        .map(|value| self.list.remove(&(slot, value)).unwrap_or_default())
                        .collect()
                })
                .collect(),
        };
        validate_basic_card_entry(&entry).ok()?;
        Some(entry)
    }
}

pub fn decode_basic_card(text: &str) -> Result<ParsedGtd<BasicCardDocument>> {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut current: Option<BasicCardBuilder> = None;
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        if line.trim() == "~" {
            continue;
        }
        if is_ignored_line(line) {
            continue;
        }
        let f = fields(line);
        if f.is_empty() {
            continue;
        }
        if f[0] == "VNUM" {
            if let Some(old) = current.take() {
                if let Some(e) = old.finish() {
                    entries.push(e)
                } else {
                    warnings.push(warning(row, "incomplete BCard entry"))
                }
            }
            current = Some(BasicCardBuilder::new());
        }
        if f[0] == "END" {
            continue;
        }
        let Some(c) = current.as_mut() else {
            warnings.push(warning(row, "BCard row outside entry"));
            continue;
        };
        match f[0] {
            "VNUM" => c.vnum = one(&f),
            "ICON" => c.icon = one(&f),
            "NAME" if f.len() >= 2 => c.name = Some(f[1..].join(" ")),
            "DESC" => {
                if let Some(v) = values(&f[1..]) {
                    c.description = Some(v)
                } else {
                    warnings.push(warning(row, "BCard DESC must contain integers"));
                }
            }
            tag if tag.starts_with("SUBJ") => match tag[4..].parse::<usize>() {
                Ok(i) if i > 0 => {
                    c.subjects
                        .insert(i - 1, f.get(1..).unwrap_or_default().join(" "));
                }
                _ => {}
            },
            tag if tag.starts_with("LIST") => {
                let p = tag[4..]
                    .split('-')
                    .filter_map(|x| x.parse::<usize>().ok())
                    .collect::<Vec<_>>();
                if p.len() == 2 && p[0] > 0 && (1..=2).contains(&p[1]) {
                    c.list.insert(
                        (p[0] - 1, p[1] - 1),
                        f.get(1..).unwrap_or_default().join(" "),
                    );
                }
            }
            _ => warnings.push(warning(row, "unrecognized BCard row")),
        }
    }
    if let Some(old) = current {
        if let Some(entry) = old.finish() {
            entries.push(entry)
        } else {
            warnings.push(warning(text.lines().count(), "incomplete BCard entry"))
        }
    }
    Ok(ParsedGtd {
        document: BasicCardDocument { entries },
        warnings,
    })
}

pub fn encode_basic_card(document: &BasicCardDocument) -> Result<String> {
    let mut out = String::new();
    for e in &document.entries {
        validate_basic_card_entry(e)?;
        push_values(&mut out, "VNUM", &[e.vnum]);
        push_values(&mut out, "ICON", &[e.icon]);
        push_text(&mut out, "NAME", &e.name);
        push_values(&mut out, "DESC", &e.description);
        for (i, s) in e.subjects.iter().enumerate() {
            push_text(&mut out, &format!("SUBJ{}", i + 1), s)
        }
        for (i, r) in e.list.iter().enumerate() {
            exact(r, 2, "BCard LIST row")?;
            for (j, s) in r.iter().enumerate() {
                push_text(&mut out, &format!("LIST{}-{}", i + 1, j + 1), s)
            }
        }
        out.push_str("END\n");
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardEntry {
    pub vnum: i32,
    pub name: String,
    pub group: Vec<i32>,
    pub style: Vec<i32>,
    pub effect: Vec<i32>,
    pub time: Vec<i32>,
    pub first_stage: Vec<i32>,
    pub second_stage: Vec<i32>,
    pub last: Vec<i32>,
    pub description: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardDocument {
    pub kits: Vec<Vec<String>>,
    pub extra_texts: Vec<String>,
    pub entries: Vec<CardEntry>,
}

pub fn decode_card(text: &str) -> Result<ParsedGtd<CardDocument>> {
    let mut kits = vec![vec![String::new(); 5]; 3];
    let mut extra_texts = vec![String::new(); 20];
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut map = std::collections::BTreeMap::<String, Vec<i32>>::new();
    let mut strings = std::collections::BTreeMap::<String, String>::new();
    let finish = |map: &mut std::collections::BTreeMap<String, Vec<i32>>,
                  strings: &mut std::collections::BTreeMap<String, String>|
     -> Option<CardEntry> {
        Some(CardEntry {
            vnum: *map.remove("VNUM")?.first()?,
            name: strings.remove("NAME")?,
            group: map.remove("GROUP")?,
            style: map.remove("STYLE")?,
            effect: map.remove("EFFECT")?,
            time: map.remove("TIME")?,
            first_stage: map.remove("1ST")?,
            second_stage: map.remove("2ST")?,
            last: map.remove("LAST")?,
            description: strings.remove("DESC")?,
        })
    };
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        if matches!(line.trim(), "END" | "~") {
            continue;
        }
        if is_ignored_line(line) {
            continue;
        }
        let f = fields(line);
        if f.is_empty() {
            continue;
        }
        if f[0] == "VNUM" && map.contains_key("VNUM") {
            if let Some(entry) = finish(&mut map, &mut strings) {
                entries.push(entry);
            } else {
                warnings.push(warning(row, "incomplete Card entry"));
                map.clear();
                strings.clear();
            }
        }
        match f[0] {
            "KIT" if f.len() >= 3 => {
                match (parse_i32(f[1]), parse_i32(f[2])) {
                    (Some(a), Some(b)) if (0..3).contains(&a) && (0..5).contains(&b) => {
                        kits[a as usize][b as usize] = f.get(3..).unwrap_or_default().join(" ");
                        continue;
                    }
                    _ => {}
                }
                warnings.push(warning(row, "invalid KIT row"))
            }
            "Z_ETC" if f.len() >= 2 => {
                match parse_i32(f[1]) {
                    Some(i) if (0..20).contains(&i) => {
                        extra_texts[i as usize] = f.get(2..).unwrap_or_default().join(" ");
                        continue;
                    }
                    _ => {}
                }
                warnings.push(warning(row, "invalid Z_ETC row"))
            }
            "NAME" | "DESC" => {
                strings.insert(f[0].into(), f.get(1..).unwrap_or_default().join(" "));
            }
            "VNUM" | "GROUP" | "STYLE" | "EFFECT" | "TIME" | "1ST" | "2ST" | "LAST" => {
                let expected = match f[0] {
                    "VNUM" => 1,
                    "GROUP" | "TIME" | "LAST" => 2,
                    "STYLE" => f.len().saturating_sub(1),
                    "1ST" => 18,
                    "2ST" => 12,
                    "EFFECT" => f.len().saturating_sub(1),
                    _ => unreachable!(),
                };
                let parsed = numeric_row(&f, expected);
                if let Some(v) = parsed {
                    map.insert(f[0].into(), v);
                } else {
                    warnings.push(warning(row, "invalid Card numeric row or arity"))
                }
            }
            _ => warnings.push(warning(row, "unrecognized Card row")),
        }
    }
    if map.contains_key("VNUM") {
        if let Some(entry) = finish(&mut map, &mut strings) {
            entries.push(entry);
        } else {
            warnings.push(warning(text.lines().count(), "incomplete Card entry"));
        }
    }
    Ok(ParsedGtd {
        document: CardDocument {
            kits,
            extra_texts,
            entries,
        },
        warnings,
    })
}

pub fn encode_card(d: &CardDocument) -> Result<String> {
    exact(&d.kits, 3, "Card KIT")?;
    exact(&d.extra_texts, 20, "Card Z_ETC")?;
    let mut out = String::from("END\n");
    for (i, r) in d.kits.iter().enumerate() {
        exact(r, 5, "Card KIT row")?;
        for (j, s) in r.iter().enumerate() {
            push_text(&mut out, "KIT", &format!("{i}\t{j}\t{s}"))
        }
    }
    for (i, s) in d.extra_texts.iter().enumerate() {
        push_text(&mut out, "Z_ETC", &format!("{i}\t{s}"))
    }
    for e in &d.entries {
        exact(&e.group, 2, "Card GROUP")?;
        exact(&e.time, 2, "Card TIME")?;
        exact(&e.first_stage, 18, "Card 1ST")?;
        exact(&e.second_stage, 12, "Card 2ST")?;
        exact(&e.last, 2, "Card LAST")?;
        push_values(&mut out, "VNUM", &[e.vnum]);
        push_text(&mut out, "NAME", &e.name);
        push_values(&mut out, "GROUP", &e.group);
        push_values(&mut out, "STYLE", &e.style);
        push_values(&mut out, "EFFECT", &e.effect);
        push_values(&mut out, "TIME", &e.time);
        push_values(&mut out, "1ST", &e.first_stage);
        push_values(&mut out, "2ST", &e.second_stage);
        push_values(&mut out, "LAST", &e.last);
        push_text(&mut out, "DESC", &e.description);
        out.push_str("END\n")
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemEntry {
    pub vnum: i32,
    pub price: i32,
    pub name: String,
    pub index: Vec<i32>,
    #[serde(rename = "type")]
    pub item_type: Vec<i32>,
    pub flags: Vec<i32>,
    pub data: Vec<i32>,
    pub buffs: Vec<Vec<i32>>,
    pub line_desc_count: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
document!(ItemDocument, ItemEntry);

pub fn decode_item(text: &str) -> Result<ParsedGtd<ItemDocument>> {
    #[derive(Clone, Copy)]
    enum DescriptionState {
        None,
        First { append_client_rows: bool },
        Additional { remaining: usize },
    }

    fn finish(
        n: &mut std::collections::BTreeMap<String, Vec<i32>>,
        name: &mut Option<String>,
        description: &mut Option<String>,
    ) -> Option<ItemEntry> {
        let vnum = n.remove("VNUM")?;
        Some(ItemEntry {
            vnum: *vnum.first()?,
            price: *vnum.get(1)?,
            name: name.take()?,
            index: n.remove("INDEX")?,
            item_type: n.remove("TYPE")?,
            flags: n.remove("FLAG")?,
            data: n.remove("DATA")?,
            buffs: chunks(n.remove("BUFF")?, 5),
            line_desc_count: *n.remove("LINEDESC")?.first()?,
            description: description.take(),
        })
    }

    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut n = std::collections::BTreeMap::<String, Vec<i32>>::new();
    let mut name = None;
    let mut desc = None;
    let mut description_state = DescriptionState::None;
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        let t = line.trim();
        match description_state {
            DescriptionState::First { append_client_rows } => {
                if t == "END" {
                    description_state = DescriptionState::None;
                    continue;
                }
                if !append_client_rows && line.starts_with('#') {
                    continue;
                }
                if !append_client_rows && (t == "~" || fields(line).first() == Some(&"VNUM")) {
                    description_state = DescriptionState::None;
                } else {
                    desc = Some(t.to_owned());
                    description_state = if append_client_rows {
                        DescriptionState::Additional { remaining: 100 }
                    } else {
                        DescriptionState::None
                    };
                    continue;
                }
            }
            DescriptionState::Additional { remaining } => {
                if line.starts_with('#') || t == "END" {
                    description_state = DescriptionState::None;
                    continue;
                }
                let description = desc.get_or_insert_default();
                description.push('\n');
                description.push_str(t);
                description_state = if remaining == 1 {
                    DescriptionState::None
                } else {
                    DescriptionState::Additional {
                        remaining: remaining - 1,
                    }
                };
                continue;
            }
            DescriptionState::None => {}
        }
        if matches!(t, "END" | "~") {
            continue;
        }
        if is_ignored_line(line) {
            continue;
        }
        let f = fields(line);
        if f.is_empty() {
            continue;
        }
        if f[0] == "VNUM" && n.contains_key("VNUM") {
            if let Some(entry) = finish(&mut n, &mut name, &mut desc) {
                entries.push(entry);
            } else {
                warnings.push(warning(row, "incomplete Item entry"));
                n.clear();
                name = None;
                desc = None;
            }
            description_state = DescriptionState::None;
        }
        match f[0] {
            "VNUM" | "INDEX" | "TYPE" | "FLAG" | "DATA" | "BUFF" | "LINEDESC" => {
                let expected = match f[0] {
                    "VNUM" | "TYPE" => 2,
                    "INDEX" => 6,
                    "FLAG" => f.len().saturating_sub(1),
                    "DATA" => 20,
                    "BUFF" => 25,
                    "LINEDESC" => 1,
                    _ => unreachable!(),
                };
                let parsed = numeric_row(&f, expected);
                if let Some(v) = parsed {
                    let line_desc_count = (f[0] == "LINEDESC").then(|| v[0]);
                    n.insert(f[0].into(), v);
                    if let Some(declared_count) = line_desc_count {
                        description_state = DescriptionState::First {
                            append_client_rows: declared_count > 0,
                        };
                    }
                } else {
                    warnings.push(warning(row, "invalid Item numeric row"))
                }
            }
            "NAME" => name = Some(f.get(1..).unwrap_or_default().join(" ")),
            _ => warnings.push(warning(row, "unrecognized Item row")),
        }
    }
    if n.contains_key("VNUM") {
        if let Some(entry) = finish(&mut n, &mut name, &mut desc) {
            entries.push(entry);
        } else {
            warnings.push(warning(text.lines().count(), "incomplete Item entry"));
        }
    }
    Ok(ParsedGtd {
        document: ItemDocument { entries },
        warnings,
    })
}

pub fn encode_item(d: &ItemDocument) -> Result<String> {
    let mut out = String::new();
    for e in &d.entries {
        exact(&e.index, 6, "Item INDEX")?;
        exact(&e.item_type, 2, "Item TYPE")?;
        exact(&e.data, 20, "Item DATA")?;
        exact(&e.buffs, 5, "Item BUFF")?;
        let mut flat = Vec::new();
        for b in &e.buffs {
            exact(b, 5, "Item BUFF group")?;
            flat.extend(b)
        }
        if let Some(description) = &e.description {
            if description.contains('\r') {
                return invalid("Item description contains a carriage return");
            }
            let lines = description.split('\n').collect::<Vec<_>>();
            if lines.iter().any(|line| line.trim() == "END") {
                return invalid("Item description contains its END boundary");
            }
            if e.line_desc_count > 0 {
                if lines.len() > 101 {
                    return invalid("Item description cannot contain more than 101 physical rows");
                }
                if lines.iter().skip(1).any(|line| line.starts_with('#')) {
                    return invalid("Item description continuation rows cannot start with '#'");
                }
            } else if lines.len() != 1
                || lines[0].trim().starts_with('#')
                || lines[0].trim() == "~"
                || fields(lines[0]).first().copied() == Some("VNUM")
            {
                return invalid(
                    "Item description with a non-positive LINEDESC is not one representable source row",
                );
            }
        }
        push_values(&mut out, "VNUM", &[e.vnum, e.price]);
        push_text(&mut out, "NAME", &e.name);
        push_values(&mut out, "INDEX", &e.index);
        push_values(&mut out, "TYPE", &e.item_type);
        push_values(&mut out, "FLAG", &e.flags);
        push_values(&mut out, "DATA", &e.data);
        push_values(&mut out, "BUFF", &flat);
        push_values(&mut out, "LINEDESC", &[e.line_desc_count]);
        if let Some(s) = &e.description {
            out.push_str(s);
            out.push('\n')
        }
        out.push_str("END\n")
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonsterEntry {
    pub vnum: i32,
    pub name: String,
    pub level: Vec<i32>,
    pub race: Vec<i32>,
    pub attributes: Vec<i32>,
    pub hp_mp: Vec<i32>,
    pub experience: Vec<i32>,
    pub pre_attack: Vec<i32>,
    pub settings: Vec<i32>,
    pub etc: Vec<i32>,
    pub pet_info: Vec<i32>,
    pub effects: Vec<i32>,
    pub z_skills: Vec<i32>,
    pub weapon_info: Vec<i32>,
    pub weapon: Vec<i32>,
    pub armor_info: Vec<i32>,
    pub armor: Vec<i32>,
    pub skills: Vec<Vec<i32>>,
    pub partner: Vec<i32>,
    pub basic: Vec<Vec<i32>>,
    pub cards: Vec<Vec<i32>>,
    pub mode: Vec<i32>,
    pub items: Vec<Vec<i32>>,
}
document!(MonsterDocument, MonsterEntry);

fn monster_row_has_arbitrary_width(tag: &str) -> bool {
    matches!(
        tag,
        "RACE" | "SETTING" | "ETC" | "PETINFO" | "MODE" | "ITEM"
    )
}

const MONSTER_TAGS: [(&str, usize); 21] = [
    ("LEVEL", 1),
    ("RACE", 3),
    ("ATTRIB", 6),
    ("HP/MP", 2),
    ("EXP", 2),
    ("PREATT", 5),
    ("SETTING", 6),
    ("ETC", 8),
    ("PETINFO", 5),
    ("EFF", 3),
    ("ZSKILL", 7),
    ("WINFO", 3),
    ("WEAPON", 7),
    ("AINFO", 2),
    ("ARMOR", 5),
    ("SKILL", 15),
    ("PARTNER", 20),
    ("BASIC", 50),
    ("CARD", 20),
    ("MODE", 32),
    ("ITEM", 60),
];
pub fn decode_monster(text: &str) -> Result<ParsedGtd<MonsterDocument>> {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut n = std::collections::BTreeMap::<String, Vec<i32>>::new();
    let mut name = None;
    fn finish(
        n: &mut std::collections::BTreeMap<String, Vec<i32>>,
        name: &mut Option<String>,
    ) -> Option<MonsterEntry> {
        Some(MonsterEntry {
            vnum: *n.remove("VNUM")?.first()?,
            name: name.take()?,
            level: n.remove("LEVEL")?,
            race: n.remove("RACE")?,
            attributes: n.remove("ATTRIB")?,
            hp_mp: n.remove("HP/MP")?,
            experience: n.remove("EXP")?,
            pre_attack: n.remove("PREATT")?,
            settings: n.remove("SETTING")?,
            etc: n.remove("ETC")?,
            pet_info: n.remove("PETINFO")?,
            effects: n.remove("EFF")?,
            z_skills: n.remove("ZSKILL")?,
            weapon_info: n.remove("WINFO")?,
            weapon: n.remove("WEAPON")?,
            armor_info: n.remove("AINFO")?,
            armor: n.remove("ARMOR")?,
            skills: chunks(n.remove("SKILL")?, 3),
            partner: n.remove("PARTNER")?,
            basic: chunks(n.remove("BASIC")?, 5),
            cards: chunks(n.remove("CARD")?, 5),
            mode: n.remove("MODE")?,
            items: chunks(n.remove("ITEM")?, 3),
        })
    }
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        if matches!(line.trim(), "END" | "~") {
            continue;
        }
        if is_ignored_line(line) {
            continue;
        }
        let f = fields(line);
        if f.is_empty() {
            continue;
        }
        if f[0] == "VNUM" && n.contains_key("VNUM") {
            if let Some(e) = finish(&mut n, &mut name) {
                entries.push(e)
            } else {
                warnings.push(warning(row, "incomplete monster entry"));
                n.clear();
                name = None
            }
        }
        match f[0] {
            "NAME" => name = Some(f.get(1..).unwrap_or_default().join(" ")),
            tag if tag == "VNUM" || MONSTER_TAGS.iter().any(|x| x.0 == tag) => {
                let expected = if tag == "VNUM" {
                    1
                } else {
                    MONSTER_TAGS.iter().find(|x| x.0 == tag).unwrap().1
                };
                let parsed = if monster_row_has_arbitrary_width(tag) {
                    values(&f[1..])
                } else {
                    numeric_row(&f, expected)
                };
                if let Some(v) = parsed {
                    n.insert(tag.into(), v);
                } else {
                    warnings.push(warning(row, "invalid monster numeric row"))
                }
            }
            _ => warnings.push(warning(row, "unrecognized monster row")),
        }
    }
    if n.contains_key("VNUM") {
        if let Some(e) = finish(&mut n, &mut name) {
            entries.push(e)
        } else {
            warnings.push(warning(text.lines().count(), "incomplete monster entry"))
        }
    }
    Ok(ParsedGtd {
        document: MonsterDocument { entries },
        warnings,
    })
}

pub fn encode_monster(d: &MonsterDocument) -> Result<String> {
    let mut out = String::new();
    for e in &d.entries {
        push_values(&mut out, "VNUM", &[e.vnum]);
        push_text(&mut out, "NAME", &e.name);
        let groups: [(&str, &Vec<i32>); 15] = [
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
        for (tag, v) in groups {
            if !monster_row_has_arbitrary_width(tag) {
                let len = MONSTER_TAGS.iter().find(|x| x.0 == tag).unwrap().1;
                exact(v, len, tag)?;
            }
            push_values(&mut out, tag, v)
        }
        exact(&e.skills, 5, "SKILL")?;
        let mut skills = Vec::new();
        for row in &e.skills {
            exact(row, 3, "SKILL")?;
            skills.extend(row);
        }
        push_values(&mut out, "SKILL", &skills);
        exact(&e.partner, 20, "PARTNER")?;
        push_values(&mut out, "PARTNER", &e.partner);
        for (tag, rows, count, width) in [("BASIC", &e.basic, 10, 5), ("CARD", &e.cards, 4, 5)] {
            exact(rows, count, tag)?;
            let mut flat = Vec::new();
            for r in rows {
                exact(r, width, tag)?;
                flat.extend(r)
            }
            push_values(&mut out, tag, &flat)
        }
        push_values(&mut out, "MODE", &e.mode);
        let mut items = Vec::new();
        for row in &e.items {
            items.extend(row);
        }
        push_values(&mut out, "ITEM", &items)
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillDescription {
    pub declared_count: i32,
    pub lines: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillEntry {
    pub vnum: i32,
    pub name: String,
    #[serde(rename = "type")]
    pub skill_type: Vec<i32>,
    pub cost: Vec<i32>,
    pub level: Vec<i32>,
    pub effect: Vec<i32>,
    pub target: Vec<i32>,
    pub data: Vec<i32>,
    pub basic: Vec<Vec<i32>>,
    pub final_combo: Vec<i32>,
    pub cell: Vec<i32>,
    pub description: SkillDescription,
}
document!(SkillDocument, SkillEntry);

fn skill_row_has_arbitrary_width(tag: &str) -> bool {
    matches!(tag, "COST" | "EFFECT" | "CELL")
}

pub fn decode_skill(text: &str) -> Result<ParsedGtd<SkillDocument>> {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut n = std::collections::BTreeMap::<String, Vec<i32>>::new();
    let mut name = None;
    let mut basic = Vec::new();
    let mut desc: Option<SkillDescription> = None;
    let mut description_rows_remaining = None;
    fn finish(
        n: &mut std::collections::BTreeMap<String, Vec<i32>>,
        name: &mut Option<String>,
        basic: &mut Vec<Vec<i32>>,
        desc: &mut Option<SkillDescription>,
    ) -> Option<SkillEntry> {
        Some(SkillEntry {
            vnum: *n.remove("VNUM")?.first()?,
            name: name.take()?,
            skill_type: n.remove("TYPE")?,
            cost: n.remove("COST")?,
            level: n.remove("LEVEL")?,
            effect: n.remove("EFFECT")?,
            target: n.remove("TARGET")?,
            data: n.remove("DATA")?,
            basic: std::mem::take(basic),
            final_combo: n.remove("FCOMBO")?,
            cell: n.remove("CELL")?,
            description: desc.take()?,
        })
    }
    for (index, line) in text.lines().enumerate() {
        let row = index + 1;
        if let Some(remaining) = description_rows_remaining {
            if remaining < 101 && line.starts_with('#') {
                description_rows_remaining = None;
                continue;
            }
            desc.as_mut().unwrap().lines.push(line.trim().to_owned());
            description_rows_remaining = if remaining == 1 {
                None
            } else {
                Some(remaining - 1)
            };
            continue;
        }
        if matches!(line.trim(), "END" | "~") {
            continue;
        }
        if is_ignored_line(line) {
            continue;
        }
        let f = fields(line);
        if f.is_empty() {
            continue;
        }
        if f[0] == "VNUM" && n.contains_key("VNUM") {
            if let Some(e) = finish(&mut n, &mut name, &mut basic, &mut desc) {
                entries.push(e)
            } else {
                warnings.push(warning(row, "incomplete Skill entry"));
                n.clear();
                name = None;
                basic.clear();
                desc = None
            }
        }
        match f[0] {
            "NAME" => name = Some(f.get(1..).unwrap_or_default().join(" ")),
            "BASIC" => {
                if let Some(v) = numeric_row(&f, 6) {
                    basic.push(v)
                } else {
                    warnings.push(warning(row, "invalid Skill BASIC row"))
                }
            }
            "Z_DESC" => {
                if let Some(v) = one(&f) {
                    desc = Some(SkillDescription {
                        declared_count: v,
                        lines: Vec::new(),
                    });
                    description_rows_remaining = (v > 0).then_some(101)
                } else {
                    warnings.push(warning(row, "invalid Skill Z_DESC row"))
                }
            }
            "VNUM" | "TYPE" | "COST" | "LEVEL" | "EFFECT" | "TARGET" | "DATA" | "FCOMBO"
            | "CELL" => {
                let expected = match f[0] {
                    "VNUM" => 1,
                    "TYPE" => 6,
                    "COST" => 33,
                    "LEVEL" | "TARGET" => 5,
                    "EFFECT" => 9,
                    "DATA" => 15,
                    "FCOMBO" => 16,
                    "CELL" => 93,
                    _ => unreachable!(),
                };
                let parsed = if skill_row_has_arbitrary_width(f[0]) {
                    values(&f[1..])
                } else {
                    numeric_row(&f, expected)
                };
                if let Some(v) = parsed {
                    n.insert(f[0].into(), v);
                } else {
                    warnings.push(warning(row, "invalid Skill numeric row"))
                }
            }
            _ => warnings.push(warning(row, "unrecognized Skill row")),
        }
    }
    if n.contains_key("VNUM") {
        if let Some(e) = finish(&mut n, &mut name, &mut basic, &mut desc) {
            entries.push(e)
        } else {
            warnings.push(warning(text.lines().count(), "incomplete Skill entry"))
        }
    }
    Ok(ParsedGtd {
        document: SkillDocument { entries },
        warnings,
    })
}

pub fn encode_skill(d: &SkillDocument) -> Result<String> {
    let mut out = String::new();
    for e in &d.entries {
        if e.description.declared_count <= 0 && !e.description.lines.is_empty() {
            return invalid("Skill Z_DESC with a non-positive count cannot contain lines");
        }
        if e.description.declared_count > 0 && e.description.lines.is_empty() {
            return invalid("Skill Z_DESC with a positive count requires a description row");
        }
        if e.description.lines.len() > 101 {
            return invalid("Skill Z_DESC cannot contain more than 101 physical rows");
        }
        if e.description
            .lines
            .iter()
            .skip(1)
            .any(|line| line.starts_with('#'))
        {
            return invalid("Skill Z_DESC continuation rows cannot start with '#'");
        }
        if e.description
            .lines
            .iter()
            .any(|line| line.contains(['\r', '\n']))
        {
            return invalid("Skill Z_DESC line contains a physical line break");
        }
        for (v, len, tag) in [
            (&e.skill_type, 6, "TYPE"),
            (&e.cost, 33, "COST"),
            (&e.level, 5, "LEVEL"),
            (&e.effect, 9, "EFFECT"),
            (&e.target, 5, "TARGET"),
            (&e.data, 15, "DATA"),
            (&e.final_combo, 16, "FCOMBO"),
            (&e.cell, 93, "CELL"),
        ] {
            if !skill_row_has_arbitrary_width(tag) {
                exact(v, len, tag)?
            }
        }
        exact(&e.basic, 5, "Skill BASIC")?;
        push_values(&mut out, "VNUM", &[e.vnum]);
        push_text(&mut out, "NAME", &e.name);
        for (tag, v) in [
            ("TYPE", &e.skill_type),
            ("COST", &e.cost),
            ("LEVEL", &e.level),
            ("EFFECT", &e.effect),
            ("TARGET", &e.target),
            ("DATA", &e.data),
        ] {
            push_values(&mut out, tag, v)
        }
        for b in &e.basic {
            exact(b, 6, "Skill BASIC row")?;
            push_values(&mut out, "BASIC", b)
        }
        push_values(&mut out, "FCOMBO", &e.final_combo);
        push_values(&mut out, "CELL", &e.cell);
        push_values(&mut out, "Z_DESC", &[e.description.declared_count]);
        for line in &e.description.lines {
            out.push_str(line);
            out.push('\n')
        }
        out.push_str("#\n")
    }
    Ok(out)
}

fn one(f: &[&str]) -> Option<i32> {
    if f.len() == 2 { parse_i32(f[1]) } else { None }
}
fn numeric_row(f: &[&str], expected: usize) -> Option<Vec<i32>> {
    (f.len() == expected + 1).then(|| values(&f[1..])).flatten()
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

    fn basic_card_entry(slot_count: usize) -> BasicCardEntry {
        BasicCardEntry {
            vnum: 4,
            icon: -1,
            name: "zts39e".to_owned(),
            description: (1..=slot_count).map(|value| value as i32).collect(),
            subjects: (1..=slot_count)
                .map(|index| format!("subject-{index}"))
                .collect(),
            list: (1..=slot_count)
                .map(|index| vec![format!("positive-{index}"), format!("negative-{index}")])
                .collect(),
        }
    }

    #[test]
    fn basic_card_validation_accepts_one_slot_without_padding() {
        let entry = basic_card_entry(1);
        validate_basic_card_entry(&entry).unwrap();

        assert_eq!(entry.description, [1]);
        assert_eq!(entry.subjects, ["subject-1"]);
        assert_eq!(entry.list[0], ["positive-1", "negative-1"]);
    }

    #[test]
    fn basic_card_validation_accepts_arbitrary_slot_counts() {
        let entry = basic_card_entry(7);
        validate_basic_card_entry(&entry).unwrap();

        assert_eq!(entry.description, [1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(
            entry.subjects,
            [
                "subject-1",
                "subject-2",
                "subject-3",
                "subject-4",
                "subject-5",
                "subject-6",
                "subject-7"
            ]
        );
        assert_eq!(entry.list[6], ["positive-7", "negative-7"]);
    }

    #[test]
    fn basic_card_validation_accepts_empty_slot_sequences() {
        validate_basic_card_entry(&basic_card_entry(0)).unwrap();
    }

    #[test]
    fn basic_card_validation_rejects_inconsistent_slot_groups() {
        let mut missing_subject = basic_card_entry(4);
        missing_subject.subjects.pop();
        assert!(validate_basic_card_entry(&missing_subject).is_err());

        let mut missing_list_group = basic_card_entry(4);
        missing_list_group.list.pop();
        assert!(validate_basic_card_entry(&missing_list_group).is_err());

        let mut incomplete_list_pair = basic_card_entry(4);
        incomplete_list_pair.list[2].pop();
        assert!(validate_basic_card_entry(&incomplete_list_pair).is_err());
    }

    #[test]
    fn basic_card_validation_preserves_signed_numeric_values() {
        let mut entry = basic_card_entry(1);
        entry.vnum = -7;
        entry.icon = -1;
        entry.description[0] = -32;

        validate_basic_card_entry(&entry).unwrap();
        assert_eq!(entry.vnum, -7);
        assert_eq!(entry.icon, -1);
        assert_eq!(entry.description, [-32]);
    }

    #[test]
    fn basic_card_validation_preserves_duplicates() {
        let mut entry = basic_card_entry(4);
        entry.description = vec![9, 9, -2, 9];
        entry.subjects = vec![
            "same".to_owned(),
            "same".to_owned(),
            "third".to_owned(),
            "same".to_owned(),
        ];

        validate_basic_card_entry(&entry).unwrap();
        assert_eq!(entry.description, [9, 9, -2, 9]);
        assert_eq!(entry.subjects, ["same", "same", "third", "same"]);
    }

    #[test]
    fn basic_card_validation_rejects_physical_line_breaks() {
        let mut entry = basic_card_entry(1);
        entry.list[0][1] = "split\nrow".to_owned();
        assert!(validate_basic_card_entry(&entry).is_err());
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
                group: vec![0; 2],
                style: vec![0; 5],
                effect: vec![],
                time: vec![0; 2],
                first_stage: vec![0; 18],
                second_stage: vec![0; 12],
                last: vec![0; 2],
                description: "d".into(),
            }],
        };
        assert!(encode_card(&base).is_ok());
        let mut long = base;
        long.entries[0].effect = vec![1, 2, 3, 4, 5, 6, 7];
        let native = encode_card(&long).unwrap();
        assert!(native.contains("EFFECT\t1\t2\t3\t4\t5\t6\t7\n"));
        assert_eq!(decode_card(&native).unwrap().document, long)
    }
    #[test]
    fn basic_card_preserves_subject_five() {
        let p=decode_basic_card("VNUM 1\nICON -1\nNAME n\nDESC 0 0 0 0 0\nSUBJ1 a\nSUBJ2 b\nSUBJ3 c\nSUBJ4 d\nSUBJ5 e\nLIST1-1 a\nLIST1-2 b\nLIST2-1 a\nLIST2-2 b\nLIST3-1 a\nLIST3-2 b\nLIST4-1 a\nLIST4-2 b\nLIST5-1 a\nLIST5-2 b\nEND\n").unwrap();
        assert_eq!(p.document.entries[0].subjects[4], "e");
        assert!(encode_basic_card(&p.document).unwrap().contains("SUBJ5\te"))
    }

    #[test]
    fn basic_card_preserves_arbitrary_slot_counts_and_client_tolerated_gaps() {
        let source = concat!(
            "VNUM 1\nICON -1\nNAME old\nDESC -2\n",
            "SUBJ1 subject\nLIST1-1 yes\nLIST1-2 no\nEND\n",
            "VNUM 2\nICON -1\nNAME modern\nDESC 1 2 3 4 5 99\n",
            "SUBJ1 a\nSUBJ2 b\nSUBJ3 c\nSUBJ4 d\nSUBJ5 e\n",
            "LIST1-1 a\nLIST1-2 b\nLIST2-1 a\nLIST2-2 b\n",
            "LIST3-1 a\nLIST3-2 b\nLIST4-1 a\nLIST4-2 b\nLIST5-1 a\n",
            "END\n~\n",
        );
        let parsed = decode_basic_card(source).unwrap();
        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document.entries.len(), 2);
        assert_eq!(parsed.document.entries[0].description, [-2]);
        assert_eq!(parsed.document.entries[0].subjects, ["subject"]);
        assert_eq!(parsed.document.entries[1].description, [1, 2, 3, 4, 5, 99]);
        assert_eq!(parsed.document.entries[1].list[4], ["a", ""]);
        assert_eq!(parsed.document.entries[1].list[5], ["", ""]);
        assert_eq!(
            decode_basic_card(&encode_basic_card(&parsed.document).unwrap())
                .unwrap()
                .document,
            parsed.document
        );
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
            assert_eq!(parsed.document.entries[0].style, style);
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
            assert_eq!(entry.flags, flags);
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
        assert!(entry.race.is_empty());
        assert_eq!(entry.settings, [1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(entry.etc, [-9]);
        assert_eq!(entry.pet_info, [1, 2, 3, 4, 5, 6]);
        assert_eq!(entry.mode.len(), 33);
        assert_eq!(entry.items, [vec![2000, 9000, 1], vec![-1]]);
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
        assert!(entry.cost.is_empty());
        assert_eq!(entry.effect, [-5, -4, -3, -2, -1, 0, 1, 2, 3, 4]);
        assert_eq!(entry.cell, [-1, 2]);
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
    fn item_entries_end_on_vnum_or_eof_and_keep_non_positive_descriptions() {
        let source = format!(
            "{}zero count\n{}negative count\n",
            item_record(1, 0),
            item_record(2, -7),
        );
        let parsed = decode_item(&source).unwrap();

        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document.entries.len(), 2);
        assert_eq!(parsed.document.entries[0].line_desc_count, 0);
        assert_eq!(
            parsed.document.entries[0].description.as_deref(),
            Some("zero count")
        );
        assert_eq!(parsed.document.entries[1].line_desc_count, -7);
        assert_eq!(
            parsed.document.entries[1].description.as_deref(),
            Some("negative count")
        );
        assert_eq!(
            decode_item(&encode_item(&parsed.document).unwrap())
                .unwrap()
                .document,
            parsed.document
        );
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
