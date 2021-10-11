/*!
Unicode character properties.
*/

pub use super::compose::Decompose;
#[doc(inline)]
pub use super::unicode_data::{
    BidiClass, Block, Category, ClusterBreak, JoiningType, LineBreak, Script, WordBreak,
    UNICODE_VERSION,
};

use super::compose::{compose_pair, decompose, decompose_compat};
use super::unicode_data::{
    get_record_index, MyanmarClass, Record, UseClass, BRACKETS, MIRRORS, RECORDS, SCRIPTS_BY_TAG,
    SCRIPT_COMPLEXITY, SCRIPT_NAMES, SCRIPT_TAGS,
};

use core::char::from_u32_unchecked;

const RECORD_MASK: u16 = 0x1FFF;
const BOUNDARY_SHIFT: u16 = 13;

/// Compact, constant time reference to Unicode properties for a character.
#[derive(Copy, Clone, PartialEq, Eq, Default)]
pub struct Properties(u16);

impl Properties {
    fn new(ch: u32) -> Self {
        Self(get_record_index(ch as usize) as u16)
    }

    /// Returns the category of the character.
    pub fn category(self) -> Category {
        self.record().category
    }

    /// Returns the unicode block that contains the character.
    pub fn block(self) -> Block {
        self.record().block
    }

    /// Returns the script to which the character belongs.
    pub fn script(self) -> Script {
        self.record().script
    }

    /// Returns the canonical combining class of the character.
    pub fn combining_class(self) -> u8 {
        self.record().combining_class
    }

    /// Returns the bidirectional type of the character.
    pub fn bidi_class(self) -> BidiClass {
        self.record().bidi_class
    }

    /// Returns the joining type of the character.
    pub fn joining_type(self) -> JoiningType {
        self.record().joining_type
    }

    /// Returns the cluster break property for the character.
    pub fn cluster_break(self) -> ClusterBreak {
        self.record().cluster_break
    }

    /// Returns the word break property for the character.
    pub fn word_break(self) -> WordBreak {
        self.record().word_break
    }

    /// Returns the line break property for the character.
    pub fn line_break(self) -> LineBreak {
        self.record().line_break
    }

    /// Returns true if the character is an emoji.
    pub fn is_emoji(self) -> bool {
        self.record().flags.is_emoji()
    }

    /// Returns true if the character is an extended pictographic symbol.
    pub fn is_extended_pictographic(self) -> bool {
        self.record().flags.is_extended_pictographic()
    }

    /// Returns true if the character is an opening bracket.
    pub fn is_open_bracket(self) -> bool {
        self.record().flags.is_open_bracket()
    }

    /// Returns true if the character is a closing bracket.
    pub fn is_close_bracket(self) -> bool {
        self.record().flags.is_close_bracket()
    }

    pub(crate) fn is_ignorable(self) -> bool {
        self.record().flags.is_ignorable()
    }

    pub(crate) fn is_variation_selector(self) -> bool {
        self.record().flags.is_variation_selector()
    }

    pub(crate) fn contributes_to_shaping(self) -> bool {
        self.record().flags.contributes_to_shaping()
    }

    pub(crate) fn with_boundary(mut self, b: u16) -> Self {
        self.set_boundary(b);
        self
    }

    pub(crate) fn boundary(self) -> u16 {
        self.0 >> BOUNDARY_SHIFT
    }

    pub(crate) fn set_boundary(&mut self, boundary: u16) {
        self.0 = (self.0 & RECORD_MASK) | (boundary & 0b111) << BOUNDARY_SHIFT;
    }

    pub(crate) fn use_class(self) -> (UseClass, bool, bool) {
        let r = self.record();
        (
            r.use_class,
            r.flags.needs_decomp(),
            r.flags.is_extended_pictographic(),
        )
    }

    pub(crate) fn myanmar_class(self) -> (MyanmarClass, bool) {
        let r = self.record();
        (r.myanmar_class, r.flags.is_extended_pictographic())
    }

    pub(crate) fn cluster_class(self) -> (ClusterBreak, bool) {
        let r = self.record();
        (r.cluster_break, r.flags.is_extended_pictographic())
    }

    #[inline(always)]
    fn record(self) -> &'static Record {
        // SAFETY: The inner index can only be generated by the private
        // constructor which produces an in-bounds record index.
        unsafe { RECORDS.get_unchecked((self.0 & RECORD_MASK) as usize) }
    }
}

impl From<char> for Properties {
    fn from(ch: char) -> Self {
        Self::new(ch as u32)
    }
}

impl From<&'_ char> for Properties {
    fn from(ch: &'_ char) -> Self {
        Self::new(*ch as u32)
    }
}

impl From<u32> for Properties {
    fn from(ch: u32) -> Self {
        Self::new(ch)
    }
}

impl From<&'_ u32> for Properties {
    fn from(ch: &'_ u32) -> Self {
        Self::new(*ch as u32)
    }
}

/// Trait that exposes Unicode properties for the `char` type.
pub trait Codepoint: Sized + Copy {
    /// Returns the codepoint properties.
    fn properties(self) -> Properties;

    /// Returns the category of the character.
    fn category(self) -> Category {
        self.properties().category()
    }

    /// Returns the unicode block that contains the character.
    fn block(self) -> Block {
        self.properties().block()
    }

    /// Returns the script to which the character belongs.
    fn script(self) -> Script {
        self.properties().script()
    }

    /// Returns the canonical combining class of the character.
    fn combining_class(self) -> u8 {
        self.properties().combining_class()
    }

    /// Returns the bidirectional type of the character.
    fn bidi_class(self) -> BidiClass {
        self.properties().bidi_class()
    }

    /// Returns the joining type of the character.
    fn joining_type(self) -> JoiningType {
        self.properties().joining_type()
    }

    /// Returns the cluster break property for the character.
    fn cluster_break(self) -> ClusterBreak {
        self.properties().cluster_break()
    }

    /// Returns the word break property for the character.
    fn word_break(self) -> WordBreak {
        self.properties().word_break()
    }

    /// Returns the line break property for the character.
    fn line_break(self) -> LineBreak {
        self.properties().line_break()
    }

    /// Returns true if the character is an emoji.
    fn is_emoji(self) -> bool {
        self.properties().is_emoji()
    }

    /// Returns true if the character is an extended pictographic symbol.
    fn is_extended_pictographic(self) -> bool {
        self.properties().is_extended_pictographic()
    }

    /// Returns the bracket type of the character.
    fn bracket_type(self) -> BracketType;

    /// If the character is a closing bracket, returns its opening bracket
    /// pair.
    fn opening_bracket(self) -> Option<char>;

    /// If the character is an opening bracket, returns its closing bracket
    /// pair.
    fn closing_bracket(self) -> Option<char>;

    /// Returns the mirror of the character, if any.
    fn mirror(self) -> Option<char>;

    /// Returns the composition of two characters, if any.
    fn compose(a: char, b: char) -> Option<char>;

    /// Returns the canonical decomposition of the character.
    fn decompose(self) -> Decompose;

    /// Returns the compatiblity decomposition of the character.
    fn decompose_compatible(self) -> Decompose;
}

impl Codepoint for char {
    fn properties(self) -> Properties {
        Properties::from(self)
    }

    fn bracket_type(self) -> BracketType {
        match self.closing_bracket() {
            Some(other) => BracketType::Open(other),
            _ => match self.opening_bracket() {
                Some(other) => BracketType::Close(other),
                _ => BracketType::None,
            },
        }
    }

    fn opening_bracket(self) -> Option<char> {
        let c = self as u32;
        if let Ok(idx) = BRACKETS.binary_search_by(|x| (x.1 as u32).cmp(&c)) {
            return Some(unsafe { from_u32_unchecked(BRACKETS[idx].0 as u32) });
        }
        None
    }

    fn closing_bracket(self) -> Option<char> {
        let c = self as u32;
        if let Ok(idx) = BRACKETS.binary_search_by(|x| (x.0 as u32).cmp(&c)) {
            return Some(unsafe { from_u32_unchecked(BRACKETS[idx].1 as u32) });
        }
        None
    }

    fn mirror(self) -> Option<char> {
        let c = self as u32;
        if let Ok(idx) = MIRRORS.binary_search_by(|x| (x.0 as u32).cmp(&c)) {
            return Some(unsafe { from_u32_unchecked(MIRRORS[idx].1 as u32) });
        }
        None
    }

    fn compose(a: char, b: char) -> Option<char> {
        compose_pair(a, b)
    }

    fn decompose(self) -> Decompose {
        decompose(self)
    }

    fn decompose_compatible(self) -> Decompose {
        decompose_compat(self)
    }
}

/// Bracket type of a character.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum BracketType {
    /// Not a bracket.
    None,
    /// An opening bracket with the associated closing bracket.
    Open(char),
    /// A closing bracket with the associated opening bracket.
    Close(char),
}

impl Script {
    /// Returns the script associated with the specified OpenType language
    /// tag.
    pub fn from_opentype(tag: u32) -> Option<Self> {
        match SCRIPTS_BY_TAG.binary_search_by(|x| x.0.cmp(&tag)) {
            Ok(index) => Some(SCRIPTS_BY_TAG[index].1),
            _ => None,
        }
    }

    /// Returns the name of the script.
    pub fn name(self) -> &'static str {
        SCRIPT_NAMES[self as usize]
    }

    /// Returns true if the script requires complex shaping.
    pub fn is_complex(self) -> bool {
        SCRIPT_COMPLEXITY[self as usize]
    }

    /// Returns true if the script has cursive joining.
    pub fn is_joined(self) -> bool {
        matches!(
            self,
            Script::Arabic
                | Script::Mongolian
                | Script::Syriac
                | Script::Nko
                | Script::PhagsPa
                | Script::Mandaic
                | Script::Manichaean
                | Script::PsalterPahlavi
                | Script::Adlam
        )
    }

    /// Returns the script as an OpenType tag.
    pub fn to_opentype(self) -> u32 {
        SCRIPT_TAGS[self as usize]
    }
}

impl WordBreak {
    pub(crate) const fn mask(self) -> u32 {
        1 << (self as u32)
    }
}

impl BidiClass {
    /// Returns the bidi class as a 32 bit bitmask.
    pub const fn mask(self) -> u32 {
        1 << (self as u32)
    }

    /// Returns true if the presence of this bidi class requires
    /// resolution.
    pub fn needs_resolution(self) -> bool {
        use BidiClass::*;
        const OVERRIDE_MASK: u32 = RLE.mask() | LRE.mask() | RLO.mask() | LRO.mask();
        const ISOLATE_MASK: u32 = RLI.mask() | LRI.mask() | FSI.mask();
        const EXPLICIT_MASK: u32 = OVERRIDE_MASK | ISOLATE_MASK;
        const BIDI_MASK: u32 = EXPLICIT_MASK | R.mask() | AL.mask() | AN.mask();
        self.mask() & BIDI_MASK != 0
    }
}