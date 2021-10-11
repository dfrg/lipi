use super::JoiningType;
use super::{CharInfo, UserData};

/// Character output from the cluster parser.
#[derive(Copy, Clone, Debug)]
pub struct Char {
    /// The character.
    pub ch: char,
    /// Offset of the character in code units.
    pub offset: usize,
    /// Shaping class of the character.
    pub shape_class: ShapeClass,
    /// Joining type of the character.
    pub joining_type: JoiningType,
    /// True if the character is ignorable.
    pub ignorable: bool,
    /// True if the character should be considered when mapping glyphs.
    pub contributes_to_shaping: bool,
    /// Nominal glyph identifier.
    pub glyph_id: u32,
    /// Arbitrary user data.
    pub data: UserData,
}

impl Default for Char {
    fn default() -> Self {
        Self {
            ch: '\0',
            shape_class: ShapeClass::Base,
            joining_type: JoiningType::U,
            ignorable: false,
            contributes_to_shaping: true,
            glyph_id: 0,
            data: 0,
            offset: 0,
        }
    }
}

/// Shaping class of a character.
#[derive(Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ShapeClass {
    /// Reph form.
    Reph,
    /// Pre-base form.
    Pref,
    /// Myanmar three character prefix.
    Kinzi,
    /// Base character.
    Base,
    /// Mark character.
    Mark,
    /// Halant modifier.
    Halant,
    /// Medial consonant Ra.
    MedialRa,
    /// Pre-base vowel modifier.
    VmPre,
    /// Pre-base dependent vowel.
    VPre,
    /// Below base dependent vowel.
    VBlw,
    /// Anusvara class.
    Anusvara,
    /// Zero width joiner.
    Zwj,
    /// Zero width non-joiner.
    Zwnj,
    /// Control character.
    Control,
    /// Variation selector.
    Vs,
    /// Other character.
    Other,
}

impl Default for ShapeClass {
    fn default() -> Self {
        Self::Base
    }
}

/// Character input to the cluster parser.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct SourceChar {
    /// The character.
    pub ch: char,
    /// Offset of the character in code units.
    pub offset: usize,
    /// Length of the character in code units.
    pub len: u8,
    /// Character information.
    pub info: CharInfo,
    /// Arbitrary user data.
    pub data: UserData,
}

impl Default for SourceChar {
    fn default() -> Self {
        Self {
            ch: '\0',
            offset: 0,
            len: 1,
            info: Default::default(),
            data: 0,
        }
    }
}
