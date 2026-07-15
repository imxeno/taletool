//! Adapters for records stored in `NSgtdData.NOS`.

mod entity;
mod localized;
mod structured;

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::{
    Result, TextEncoding, TextError, decode_dat_payload, encode_dat_payload, encode_legacy_text,
};

pub use entity::*;
pub use localized::*;
pub use structured::*;

/// Locale carried by localized NSgtdData record names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GtdLocale {
    Cz,
    De,
    Es,
    Fr,
    Gsp,
    Hk,
    In,
    It,
    Jp,
    Kr,
    My,
    Pl,
    Ru,
    Tr,
    Tw,
    Uk,
}

/// Semantic grammar selected from an NSgtdData record name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GtdFileKind {
    ActDescription,
    BasicCard,
    Card,
    Item,
    Monster,
    NpcTalk,
    Skill,
    Quest,
    QuestPrize,
    Tutorial,
    ShopType,
    MapId,
    MapPoint,
    QuestNpc,
    Team,
    Fish,
    NosMall(GtdLocale),
    Abuse(GtdLocale),
}

impl GtdFileKind {
    /// Infer a grammar from a native payload filename.
    pub fn for_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?.to_ascii_lowercase();
        let fixed = match name.as_str() {
            "act_desc.dat" => Some(Self::ActDescription),
            "bcard.dat" => Some(Self::BasicCard),
            "card.dat" => Some(Self::Card),
            "item.dat" => Some(Self::Item),
            "monster.dat" => Some(Self::Monster),
            "npctalk.dat" => Some(Self::NpcTalk),
            "skill.dat" => Some(Self::Skill),
            "quest.dat" => Some(Self::Quest),
            "qstprize.dat" => Some(Self::QuestPrize),
            "tutorial.dat" => Some(Self::Tutorial),
            "shoptype.dat" => Some(Self::ShopType),
            "mapiddata.dat" => Some(Self::MapId),
            "mappointdata.dat" => Some(Self::MapPoint),
            "qstnpc.dat" => Some(Self::QuestNpc),
            "team.dat" => Some(Self::Team),
            "fish.dat" => Some(Self::Fish),
            _ => None,
        };
        if fixed.is_some() {
            return fixed;
        }

        if let Some(locale) = name.strip_suffix("_nosmall.dat") {
            return GtdLocale::for_code(locale).map(Self::NosMall);
        }
        if let Some(locale) = name.strip_suffix("_abuse.lst") {
            return GtdLocale::for_code(locale).map(Self::Abuse);
        }
        None
    }

    /// Default legacy encoding used by this source record.
    pub const fn default_encoding(self) -> TextEncoding {
        let locale = match self {
            Self::NosMall(locale) | Self::Abuse(locale) => Some(locale),
            _ => None,
        };
        match locale {
            Some(GtdLocale::Cz | GtdLocale::De | GtdLocale::It | GtdLocale::Pl) => {
                TextEncoding::Windows1250
            }
            Some(
                GtdLocale::Es
                | GtdLocale::Fr
                | GtdLocale::Gsp
                | GtdLocale::In
                | GtdLocale::My
                | GtdLocale::Uk,
            ) => TextEncoding::Windows1252,
            Some(GtdLocale::Ru) => TextEncoding::Windows1251,
            Some(GtdLocale::Tr) => TextEncoding::Windows1254,
            Some(GtdLocale::Hk | GtdLocale::Tw) => TextEncoding::Big5,
            Some(GtdLocale::Jp) => TextEncoding::ShiftJis,
            Some(GtdLocale::Kr) | None => TextEncoding::EucKr,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ActDescription => "act_description",
            Self::BasicCard => "basic_card",
            Self::Card => "card",
            Self::Item => "item",
            Self::Monster => "monster",
            Self::NpcTalk => "npc_talk",
            Self::Skill => "skill",
            Self::Quest => "quest",
            Self::QuestPrize => "quest_prize",
            Self::Tutorial => "tutorial",
            Self::ShopType => "shop_type",
            Self::MapId => "map_id",
            Self::MapPoint => "map_point",
            Self::QuestNpc => "quest_npc",
            Self::Team => "team",
            Self::Fish => "fish",
            Self::NosMall(_) => "nos_mall",
            Self::Abuse(_) => "abuse",
        }
    }
}

impl GtdLocale {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Cz => "cz",
            Self::De => "de",
            Self::Es => "es",
            Self::Fr => "fr",
            Self::Gsp => "gsp",
            Self::Hk => "hk",
            Self::In => "in",
            Self::It => "it",
            Self::Jp => "jp",
            Self::Kr => "kr",
            Self::My => "my",
            Self::Pl => "pl",
            Self::Ru => "ru",
            Self::Tr => "tr",
            Self::Tw => "tw",
            Self::Uk => "uk",
        }
    }

    pub(crate) fn for_code(code: &str) -> Option<Self> {
        match code.to_ascii_lowercase().as_str() {
            "cz" => Some(Self::Cz),
            "de" => Some(Self::De),
            "es" => Some(Self::Es),
            "fr" => Some(Self::Fr),
            "gsp" => Some(Self::Gsp),
            "hk" => Some(Self::Hk),
            "in" => Some(Self::In),
            "it" => Some(Self::It),
            "jp" => Some(Self::Jp),
            "kr" => Some(Self::Kr),
            "my" => Some(Self::My),
            "pl" => Some(Self::Pl),
            "ru" => Some(Self::Ru),
            "tr" => Some(Self::Tr),
            "tw" => Some(Self::Tw),
            "uk" => Some(Self::Uk),
            _ => None,
        }
    }
}

/// A physical source row skipped while constructing a semantic document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GtdWarning {
    pub row: usize,
    pub message: String,
}

/// A decoded semantic document together with ignored-row diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedGtd<T> {
    pub document: T,
    pub warnings: Vec<GtdWarning>,
}

/// JSON document for one supported NSgtdData record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GtdDocument {
    pub schema_version: u32,
    #[serde(flatten)]
    pub data: GtdDocumentData,
}

/// Format-specific portion of a [`GtdDocument`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GtdDocumentData {
    ActDescription(ActDescriptionDocument),
    BasicCard(BasicCardDocument),
    Card(CardDocument),
    Item(ItemDocument),
    Monster(MonsterDocument),
    NpcTalk(NpcTalkDocument),
    Skill(SkillDocument),
    Quest(QuestDocument),
    QuestPrize(QuestPrizeDocument),
    Tutorial(TutorialDocument),
    ShopType(ShopTypeDocument),
    MapId(MapIdDocument),
    MapPoint(MapPointDocument),
    QuestNpc(QuestNpcDocument),
    Team(TeamDocument),
    Fish(FishDocument),
    NosMall(NosMallDocument),
    Abuse(AbuseDocument),
}

/// A GtdDocument plus ignored-row diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedGtdDocument {
    pub document: GtdDocument,
    pub warnings: Vec<GtdWarning>,
}

impl GtdDocumentData {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ActDescription(_) => "act_description",
            Self::BasicCard(_) => "basic_card",
            Self::Card(_) => "card",
            Self::Item(_) => "item",
            Self::Monster(_) => "monster",
            Self::NpcTalk(_) => "npc_talk",
            Self::Skill(_) => "skill",
            Self::Quest(_) => "quest",
            Self::QuestPrize(_) => "quest_prize",
            Self::Tutorial(_) => "tutorial",
            Self::ShopType(_) => "shop_type",
            Self::MapId(_) => "map_id",
            Self::MapPoint(_) => "map_point",
            Self::QuestNpc(_) => "quest_npc",
            Self::Team(_) => "team",
            Self::Fish(_) => "fish",
            Self::NosMall(_) => "nos_mall",
            Self::Abuse(_) => "abuse",
        }
    }

    /// Number of top-level source records represented by this document.
    pub fn entry_count(&self) -> usize {
        match self {
            Self::ActDescription(value) => value.data.len(),
            Self::BasicCard(value) => value.entries.len(),
            Self::Card(value) => value.entries.len(),
            Self::Item(value) => value.entries.len(),
            Self::Monster(value) => value.entries.len(),
            Self::NpcTalk(value) => value.entries.len(),
            Self::Skill(value) => value.entries.len(),
            Self::Quest(value) => value.entries.len(),
            Self::QuestPrize(value) => value.entries.len(),
            Self::Tutorial(value) => value.entries.len(),
            Self::ShopType(value) => value.entries.len(),
            Self::MapId(value) => value.entries.len(),
            Self::MapPoint(value) => value.sections.len(),
            Self::QuestNpc(value) => value.rows.len(),
            Self::Team(value) => value.entries.len(),
            Self::Fish(value) => value.entries.len(),
            Self::NosMall(value) => value.entries.len(),
            Self::Abuse(value) => value.entries.len(),
        }
    }
}

impl GtdDocument {
    /// Number of top-level source records represented by this document.
    pub fn entry_count(&self) -> usize {
        self.data.entry_count()
    }
}

/// Decode a native payload selected by its source filename grammar.
pub fn decode_gtd_document(
    kind: GtdFileKind,
    payload: &[u8],
    encoding: Option<TextEncoding>,
) -> Result<ParsedGtdDocument> {
    let encoding = encoding.unwrap_or_else(|| kind.default_encoding());
    let (data, warnings) = match kind {
        GtdFileKind::NosMall(locale) => {
            let parsed = decode_nos_mall(payload, locale, encoding)?;
            (GtdDocumentData::NosMall(parsed.document), parsed.warnings)
        }
        GtdFileKind::Abuse(locale) => {
            let parsed = decode_abuse(payload, locale, encoding)?;
            (GtdDocumentData::Abuse(parsed.document), parsed.warnings)
        }
        _ => {
            let decoded = decode_dat_payload(payload)?;
            let text = super::decode_legacy_text(&decoded, encoding)?;
            match kind {
                GtdFileKind::ActDescription => {
                    let parsed = decode_act_description(&text)?;
                    (
                        GtdDocumentData::ActDescription(parsed.document),
                        parsed.warnings,
                    )
                }
                GtdFileKind::BasicCard => {
                    let parsed = decode_basic_card(&text)?;
                    (GtdDocumentData::BasicCard(parsed.document), parsed.warnings)
                }
                GtdFileKind::Card => {
                    let parsed = decode_card(&text)?;
                    (GtdDocumentData::Card(parsed.document), parsed.warnings)
                }
                GtdFileKind::Item => {
                    let parsed = decode_item(&text)?;
                    (GtdDocumentData::Item(parsed.document), parsed.warnings)
                }
                GtdFileKind::Monster => {
                    let parsed = decode_monster(&text)?;
                    (GtdDocumentData::Monster(parsed.document), parsed.warnings)
                }
                GtdFileKind::NpcTalk => {
                    let parsed = decode_npc_talk(&text)?;
                    (GtdDocumentData::NpcTalk(parsed.document), parsed.warnings)
                }
                GtdFileKind::Skill => {
                    let parsed = decode_skill(&text)?;
                    (GtdDocumentData::Skill(parsed.document), parsed.warnings)
                }
                GtdFileKind::Quest => {
                    let parsed = decode_quest(&text)?;
                    (GtdDocumentData::Quest(parsed.document), parsed.warnings)
                }
                GtdFileKind::QuestPrize => {
                    let parsed = decode_quest_prize(&text)?;
                    (
                        GtdDocumentData::QuestPrize(parsed.document),
                        parsed.warnings,
                    )
                }
                GtdFileKind::Tutorial => {
                    let parsed = decode_tutorial(&text)?;
                    (GtdDocumentData::Tutorial(parsed.document), parsed.warnings)
                }
                GtdFileKind::ShopType => {
                    let parsed = decode_shop_type(&text)?;
                    (GtdDocumentData::ShopType(parsed.document), parsed.warnings)
                }
                GtdFileKind::MapId => {
                    let parsed = decode_map_id(&text)?;
                    (GtdDocumentData::MapId(parsed.document), parsed.warnings)
                }
                GtdFileKind::MapPoint => {
                    let parsed = decode_map_point(&text)?;
                    (GtdDocumentData::MapPoint(parsed.document), parsed.warnings)
                }
                GtdFileKind::QuestNpc => {
                    let parsed = decode_quest_npc(&text)?;
                    (GtdDocumentData::QuestNpc(parsed.document), parsed.warnings)
                }
                GtdFileKind::Team => {
                    let parsed = decode_team(&text)?;
                    (GtdDocumentData::Team(parsed.document), parsed.warnings)
                }
                GtdFileKind::Fish => {
                    let parsed = decode_fish(&text)?;
                    (GtdDocumentData::Fish(parsed.document), parsed.warnings)
                }
                GtdFileKind::NosMall(_) | GtdFileKind::Abuse(_) => unreachable!(),
            }
        }
    };
    Ok(ParsedGtdDocument {
        document: GtdDocument {
            schema_version: 1,
            data,
        },
        warnings,
    })
}

/// Encode a GtdDocument into its native payload.
pub fn encode_gtd_document(
    kind: GtdFileKind,
    document: &GtdDocument,
    encoding: Option<TextEncoding>,
) -> Result<Vec<u8>> {
    if document.schema_version != 1 {
        return Err(TextError::InvalidGtdDocument {
            message: format!("unsupported schema_version {}", document.schema_version),
        });
    }
    if document.data.label() != kind.label() {
        return Err(TextError::GtdDocumentKindMismatch {
            expected: kind.label().to_owned(),
        });
    }
    let encoding = encoding.unwrap_or_else(|| kind.default_encoding());
    match (&document.data, kind) {
        (GtdDocumentData::NosMall(value), GtdFileKind::NosMall(locale)) => {
            if value.locale != locale {
                return Err(TextError::GtdDocumentKindMismatch {
                    expected: format!("nos_mall locale {}", locale.code()),
                });
            }
            encode_nos_mall(value, encoding)
        }
        (GtdDocumentData::Abuse(value), GtdFileKind::Abuse(locale)) => {
            if value.locale != locale {
                return Err(TextError::GtdDocumentKindMismatch {
                    expected: format!("abuse locale {}", locale.code()),
                });
            }
            encode_abuse(value, encoding)
        }
        (data, _) => {
            let text = match data {
                GtdDocumentData::ActDescription(value) => encode_act_description(value)?,
                GtdDocumentData::BasicCard(value) => encode_basic_card(value)?,
                GtdDocumentData::Card(value) => encode_card(value)?,
                GtdDocumentData::Item(value) => encode_item(value)?,
                GtdDocumentData::Monster(value) => encode_monster(value)?,
                GtdDocumentData::NpcTalk(value) => encode_npc_talk(value)?,
                GtdDocumentData::Skill(value) => encode_skill(value)?,
                GtdDocumentData::Quest(value) => encode_quest(value)?,
                GtdDocumentData::QuestPrize(value) => encode_quest_prize(value)?,
                GtdDocumentData::Tutorial(value) => encode_tutorial(value)?,
                GtdDocumentData::ShopType(value) => encode_shop_type(value)?,
                GtdDocumentData::MapId(value) => encode_map_id(value)?,
                GtdDocumentData::MapPoint(value) => encode_map_point(value)?,
                GtdDocumentData::QuestNpc(value) => encode_quest_npc(value)?,
                GtdDocumentData::Team(value) => encode_team(value)?,
                GtdDocumentData::Fish(value) => encode_fish(value)?,
                GtdDocumentData::NosMall(_) | GtdDocumentData::Abuse(_) => unreachable!(),
            };
            let bytes = encode_legacy_text(&text, encoding)?;
            encode_dat_payload(&bytes)
        }
    }
}

pub(crate) fn warning(row: usize, message: impl Into<String>) -> GtdWarning {
    GtdWarning {
        row,
        message: message.into(),
    }
}

pub(crate) fn parse_i32(token: &str) -> Option<i32> {
    token.parse().ok()
}

pub(crate) fn fields(line: &str) -> Vec<&str> {
    line.split(['\t', ' '])
        .filter(|field| !field.is_empty())
        .collect()
}

pub(crate) fn values(tokens: &[&str]) -> Option<Vec<i32>> {
    tokens.iter().map(|token| parse_i32(token)).collect()
}

pub(crate) fn is_ignored_line(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || line.starts_with('#')
}

pub(crate) fn push_values(out: &mut String, tag: &str, values: &[i32]) {
    out.push_str(tag);
    for value in values {
        out.push('\t');
        out.push_str(&value.to_string());
    }
    out.push('\n');
}

pub(crate) fn push_text(out: &mut String, tag: &str, text: &str) {
    out.push_str(tag);
    if !text.is_empty() {
        out.push('\t');
        out.push_str(text);
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_every_supported_native_filename() {
        let core = [
            "act_desc.dat",
            "BCard.dat",
            "Card.dat",
            "Item.dat",
            "monster.dat",
            "npctalk.dat",
            "Skill.dat",
            "quest.dat",
            "qstprize.dat",
            "tutorial.dat",
            "shoptype.dat",
            "MapIDData.dat",
            "MapPointData.dat",
            "qstnpc.dat",
            "team.dat",
            "fish.dat",
        ];
        assert!(
            core.into_iter()
                .all(|name| GtdFileKind::for_path(Path::new(name)).is_some())
        );

        let locales = [
            "cz", "de", "es", "fr", "gsp", "hk", "in", "it", "jp", "kr", "my", "pl", "ru", "tr",
            "tw", "uk",
        ];
        for locale in locales {
            assert!(GtdFileKind::for_path(Path::new(&format!("{locale}_nosmall.dat"))).is_some());
            assert!(GtdFileKind::for_path(Path::new(&format!("{locale}_abuse.lst"))).is_some());
        }
        assert_eq!(
            GtdFileKind::for_path(Path::new("jp_nosmall.dat"))
                .unwrap()
                .default_encoding(),
            TextEncoding::ShiftJis
        );
        assert_eq!(
            GtdFileKind::for_path(Path::new("tw_abuse.lst"))
                .unwrap()
                .default_encoding(),
            TextEncoding::Big5
        );
        assert_eq!(
            GtdFileKind::for_path(Path::new("gsp_nosmall.dat"))
                .unwrap()
                .default_encoding(),
            TextEncoding::Windows1252
        );
    }

    #[test]
    fn versioned_document_uses_a_flat_kind_tag() {
        let document = GtdDocument {
            schema_version: 1,
            data: GtdDocumentData::ActDescription(ActDescriptionDocument::default()),
        };
        let json = serde_json::to_value(&document).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["kind"], "act_description");
        assert!(json.get("data").is_some());
        assert!(json.get("titles").is_some());
        assert_eq!(
            serde_json::from_value::<GtdDocument>(json).unwrap(),
            document
        );
    }

    #[test]
    fn one_public_flow_preserves_arbitrary_card_style_widths() {
        let zeros = |count: usize| {
            std::iter::repeat_n("0", count)
                .collect::<Vec<_>>()
                .join(" ")
        };
        let source = |style: &str| {
            format!(
                concat!(
                    "VNUM 1\nNAME n\nGROUP 0 0\nSTYLE {style}\nEFFECT 0 0\n",
                    "TIME 0 0\n1ST {}\n2ST {}\nLAST 0 0\nDESC d\n"
                ),
                zeros(18),
                zeros(12),
                style = style,
            )
        };
        let decode = |text: String| {
            let payload = encode_dat_payload(text.as_bytes()).unwrap();
            decode_gtd_document(GtdFileKind::Card, &payload, None)
                .unwrap()
                .document
        };

        for style in ["", "1 2 3", "1 2 3 4 5 6 7"] {
            let document = decode(source(style));
            let GtdDocumentData::Card(card) = &document.data else {
                panic!("expected Card document")
            };
            assert_eq!(
                card.entries[0].style.len(),
                style.split_whitespace().count()
            );

            let payload = encode_gtd_document(GtdFileKind::Card, &document, None).unwrap();
            assert_eq!(
                decode_gtd_document(GtdFileKind::Card, &payload, None)
                    .unwrap()
                    .document,
                document
            );
        }
    }
}
