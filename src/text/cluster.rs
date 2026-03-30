//! Cluster types for text analysis.

use core::ops::{Deref, DerefMut, Range};
use {icu_properties::props::BinaryProperty, icu_segmenter::options::WordType as IcuWordType};

/// A grapheme cluster.
#[derive(Clone, Debug)]
pub struct Cluster {
    /// Cluster properties.
    pub attributes: ClusterAttributes,
    /// Range in the source text.
    pub text_range: Range<usize>,
    /// True if this cluster was replaced.
    pub is_replaced: bool,
}

impl Deref for Cluster {
    type Target = ClusterAttributes;

    fn deref(&self) -> &Self::Target {
        &self.attributes
    }
}

impl DerefMut for Cluster {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.attributes
    }
}

/// The content and segmentation state of a cluster.
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug)]
#[repr(transparent)]
pub struct ClusterAttributes(u8);

impl ClusterAttributes {
    /// Bits used for the content type.
    const CONTENT_MASK: u8 = 0b1111;

    /// Bit that stores line break state.
    const LINE_BREAK_BIT: u8 = 0b0001_0000;

    /// Bits used for the word type.
    const WORD_KIND_MASK: u8 = 0b0110_0000;

    /// Bit shift for the word type.
    const WORD_KIND_SHIFT: u8 = Self::WORD_KIND_MASK.trailing_zeros() as u8;

    /// Bit used to signify a right to left ordered cluster.
    const RTL_BIT: u8 = 0b1000_0000;

    /// Returns the content type of the cluster.
    pub const fn content(self) -> ClusterContent {
        ClusterContent::from_bits(self.0 & Self::CONTENT_MASK)
    }

    /// Returns true if there is a line break opportunity _after_ this cluster.
    pub const fn can_break_line_after(self) -> bool {
        self.0 & Self::LINE_BREAK_BIT != 0
    }

    /// Returns true if this cluster is the end of a word.
    pub const fn is_end_of_word(self) -> bool {
        self.word_bits() != 0
    }

    /// Returns a word kind if this cluster represents the _end_ of a word.    
    pub const fn word_kind(self) -> Option<WordKind> {
        match self.word_bits() {
            0 => None,
            1 => Some(WordKind::Letter),
            2 => Some(WordKind::Number),
            3 => Some(WordKind::Other),
            _ => None,
        }
    }

    /// Returns true if this cluster is ordered left to right.
    pub const fn is_ltr(self) -> bool {
        !self.is_rtl()
    }

    /// Returns true if this cluster is ordered right to left.
    pub const fn is_rtl(self) -> bool {
        self.0 & Self::RTL_BIT != 0
    }

    /// Returns true if this cluster is an emoji or symbol.
    pub const fn is_emoji_or_symbol(self) -> bool {
        let content = self.0 & Self::CONTENT_MASK;
        content == ClusterContent::Emoji as _ || content == ClusterContent::Symbol as _
    }

    /// Returns true if this cluster is any whitespace.
    pub const fn is_whitespace(self) -> bool {
        (self.0 & Self::CONTENT_MASK) >= ClusterContent::Space as _
    }

    /// Returns true if this cluster is a paragraph separator.
    pub const fn is_paragraph_separator(self) -> bool {
        (self.0 & Self::CONTENT_MASK) == ClusterContent::ParagraphSeparator as _
    }

    const fn word_bits(self) -> u8 {
        ((self.0 & Self::WORD_KIND_MASK) >> Self::WORD_KIND_SHIFT) & 0b11
    }

    pub(super) fn set_content(&mut self, content: ClusterContent) {
        self.0 = self.0 & !Self::CONTENT_MASK | (content as u8);
    }

    pub(super) fn set_line_break(&mut self) {
        self.0 |= Self::LINE_BREAK_BIT;
    }

    pub(super) fn set_word_kind(&mut self, kind: WordKind) {
        self.0 = self.0 & !Self::WORD_KIND_MASK | ((kind as u8 + 1) << Self::WORD_KIND_SHIFT);
    }

    pub(super) fn set_rtl(&mut self) {
        self.0 |= Self::RTL_BIT;
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
        if self.is_emoji_or_symbol() {
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
    /// Regional indicators or flag emojis.
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

/// A synchronized range for text and clusters.
#[derive(Clone, Default, Debug)]
pub struct ClusterRange {
    /// The range in the source text in code units.
    pub text: Range<usize>,
    /// The range in the cluster buffer.
    pub clusters: Range<usize>,
}
