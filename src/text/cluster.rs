//! Cluster types for text analysis.

use icu_properties::props::BinaryProperty;
use icu_segmenter::options::WordType as IcuWordType;

/// The content and segmentation properties of a cluster.
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug)]
#[repr(transparent)]
pub struct ClusterFlags(u8);

impl ClusterFlags {
    const CONTENT_MASK: u8 = 0b1111;
    const LINE_BREAK_MASK: u8 = 0b0001_0000;
    const WORD_KIND_MASK: u8 = 0b0110_0000;
    const WORD_KIND_SHIFT: u8 = 5;
}

impl ClusterFlags {
    /// Returns the content type of the cluster.
    pub const fn content(self) -> ClusterContent {
        ClusterContent::from_bits(self.0 & Self::CONTENT_MASK)
    }

    /// Returns true if there is a line break opportunity _after_ this cluster.
    pub const fn is_line_break_opportunity(self) -> bool {
        self.0 & Self::LINE_BREAK_MASK != 0
    }

    /// Returns a word kind if this cluster represents the _end_ of a word.    
    pub const fn word_kind(self) -> Option<WordKind> {
        let bits = (self.0 & Self::WORD_KIND_MASK) >> Self::WORD_KIND_SHIFT;
        match bits & 0b11 {
            0 => None,
            1 => Some(WordKind::Letter),
            2 => Some(WordKind::Number),
            3 => Some(WordKind::Other),
            _ => None,
        }
    }

    pub(super) const fn is_emoji(self) -> bool {
        let content = self.0 & Self::CONTENT_MASK;
        content == ClusterContent::Emoji as _ || content == ClusterContent::Symbol as _
    }

    pub(super) fn set_content(&mut self, content: ClusterContent) {
        self.0 = self.0 & !Self::CONTENT_MASK | (content as u8);
    }

    pub(super) fn set_line_break(&mut self) {
        self.0 |= Self::LINE_BREAK_MASK;
    }

    pub(super) fn set_word_kind(&mut self, kind: WordKind) {
        self.0 = self.0 & !Self::WORD_KIND_MASK | ((kind as u8 + 1) << Self::WORD_KIND_SHIFT);
    }

    pub(super) fn new(ch: char, char_props: parley_data::Properties) -> Self {
        let mut cluster = Self::default();
        use icu_properties::props;
        cluster.set_content(if super::is_paragraph_separator(ch) {
            ClusterContent::ParagraphSeparator
        } else if props::ExtendedPictographic::for_char(ch) {
            if props::EmojiPresentation::for_char(ch) {
                ClusterContent::Emoji
            } else {
                ClusterContent::Symbol
            }
        } else if char_props.is_region_indicator() {
            ClusterContent::RegionalIndicator
        } else if ch == ' ' {
            ClusterContent::Space
        } else if ch == '\u{00A0}' {
            ClusterContent::NoBreakSpace
        } else if ch == '\t' {
            ClusterContent::Tab
        } else if ch.is_whitespace() {
            ClusterContent::OtherWhitespace
        } else {
            ClusterContent::Text
        });
        cluster
    }

    pub(super) fn update_content(&mut self, ch: char) {
        const EMOJI_PRESENTATION: char = '\u{FE0F}';
        const TEXT_PRESENTATION: char = '\u{FE0E}';
        if self.is_emoji() {
            if ch == EMOJI_PRESENTATION {
                self.set_content(ClusterContent::Emoji);
            } else if ch == TEXT_PRESENTATION {
                self.set_content(ClusterContent::Symbol);
            }
        }
    }
}

/// The content of a cluster.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
#[repr(u8)]
pub enum ClusterContent {
    // Basic text.
    #[default]
    Text = 0,
    /// Emoji with emoji presentation.
    Emoji = 1,
    /// Symbol or emoji with text presentation.
    Symbol = 2,
    /// Flags.
    RegionalIndicator = 3,
    /// Basic space.
    Space = 4,
    /// Non-breaking space.
    NoBreakSpace = 5,
    /// Horizontal tab.
    Tab = 6,
    /// Any newline sequence.
    ParagraphSeparator = 7,
    /// Other whitespace.
    OtherWhitespace = 8,
}

impl ClusterContent {
    const fn from_bits(bits: u8) -> Self {
        match bits {
            0 => Self::Text,
            1 => Self::Emoji,
            2 => Self::Symbol,
            3 => Self::RegionalIndicator,
            4 => Self::Space,
            5 => Self::NoBreakSpace,
            6 => Self::Tab,
            7 => Self::ParagraphSeparator,
            8 => Self::OtherWhitespace,
            _ => Self::Text,
        }
    }
}

/// The type of a word.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum WordKind {
    /// Letter like word.
    Letter = 0,
    /// Number like word.
    Number = 1,
    /// Other type of word.
    Other = 2,
}

impl WordKind {
    pub(super) fn from_icu(wt: IcuWordType) -> Self {
        match wt {
            IcuWordType::Letter => Self::Letter,
            IcuWordType::Number => Self::Number,
            _ => Self::Other,
        }
    }
}
