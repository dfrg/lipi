//! Unicode property abstraction and default ICU/parley implementation.

pub use super::bidi::{BidiBracket, BidiClass};

use super::{TextAnalysisProperties, WordKind};
use crate::Script;

/// Backend-independent character properties used by text analysis.
#[derive(Copy, Clone, Debug)]
pub struct CharProperties {
    /// Script classification of the character (Unicode `Script`).
    pub script: Script,
    /// Bidirectional class used by UAX #9 resolution (Unicode `Bidi_Class`).
    pub bidi_class: BidiClass,
    /// Paired bracket information used by bidi bracket resolution
    /// (Unicode `Bidi_Paired_Bracket` and `Bidi_Paired_Bracket_Type`).
    pub bidi_bracket: Option<BidiBracket>,
    /// Whether this code point is a regional indicator symbol
    /// (Unicode `Regional_Indicator`).
    pub is_regional_indicator: bool,
    /// Whether this code point is an extended pictograph
    /// (Unicode `Extended_Pictographic`).
    pub is_extended_pictographic: bool,
    /// Whether emoji presentation is the default for this code point
    /// (Unicode `Emoji_Presentation`).
    pub is_emoji_presentation: bool,
}

/// Long-lived segmentation state created by a Unicode engine.
pub trait UnicodeSegmentationContext {
    /// Active cursor instance tied to a particular input slice lifetime.
    type Cursor<'s>: UnicodeSegmentationCursor<'s, Context = Self>
    where
        Self: 's;

    /// Creates a new segmentation stream over `text`.
    ///
    /// The returned segmenter produces boundaries relative to `text` and
    /// configured for the supplied analysis `properties`.
    fn cursor<'s>(&'s self, text: &'s str, properties: TextAnalysisProperties) -> Self::Cursor<'s>;
}

/// Active segmentation state over a particular text slice.
pub trait UnicodeSegmentationCursor<'s> {
    /// Long-lived context type that created this cursor.
    type Context: UnicodeSegmentationContext;

    /// Resets grapheme and word streams to operate on a new `text` slice.
    ///
    /// After reset, subsequent boundaries are relative to the new slice.
    fn reset_text_boundaries(&mut self, context: &'s Self::Context, text: &'s str);

    /// Resets line-break stream to operate on a new `text` slice.
    ///
    /// Line segmentation may depend on `properties` (for example break mode),
    /// so callers pass the current properties used for the new slice.
    fn reset_line_boundaries(
        &mut self,
        context: &'s Self::Context,
        text: &'s str,
        properties: TextAnalysisProperties,
    );

    /// Returns the next grapheme boundary at or after the current stream position.
    ///
    /// The stream is consumed monotonically. Implementations should not provide
    /// random-access boundary queries; the analyzer handles any index-based
    /// tracking and absolute offset mapping internally.
    fn next_grapheme(&mut self) -> Option<usize>;

    /// Returns the next word boundary and its word kind.
    ///
    /// The returned kind applies to the word segment that ends at the returned
    /// boundary. Like grapheme boundaries, this is a forward-only stream, and
    /// the boundary is relative to the currently segmented slice.
    fn next_word(&mut self) -> Option<(usize, WordKind)>;

    /// Returns the next line break opportunity boundary.
    ///
    /// This is a forward-only stream consumed by the analyzer, with boundaries
    /// relative to the currently segmented slice.
    fn next_line(&mut self) -> Option<usize>;
}

/// Interface for Unicode character property lookup.
pub trait UnicodeEngine {
    /// Context type used to create/reset segmentation cursors.
    type SegmentationContext: UnicodeSegmentationContext;

    /// Returns Unicode-derived properties for a single character.
    fn char_properties(&self, ch: char) -> CharProperties;

    /// Returns long-lived segmentation state used to create/reset cursors.
    fn segmentation_context(&self) -> Self::SegmentationContext;
}

#[cfg(feature = "icu")]
pub use icu::{IcuUnicodeEngine, IcuUnicodeSegmentationState, IcuUnicodeSegmenters};

#[cfg(feature = "icu")]
mod icu {
    use super::super::{LineBreak, WordBreak};
    use super::{
        BidiBracket, BidiClass, CharProperties, Script, TextAnalysisProperties, UnicodeEngine,
        UnicodeSegmentationContext, UnicodeSegmentationCursor, WordKind,
    };
    use core::convert::TryInto;
    use icu_locale_core::LanguageIdentifier;
    use icu_properties::props::{
        BidiMirroringGlyph, BidiPairedBracketType, BinaryProperty, EnumeratedProperty,
        NamedEnumeratedProperty, Script as IcuScript,
    };
    use icu_segmenter::options::{
        LineBreakOptions as IcuLineBreakOptions, LineBreakStrictness, LineBreakWordOption,
        WordBreakInvariantOptions,
    };

    #[derive(Copy, Clone, Default, Debug)]
    pub struct IcuUnicodeEngine;

    impl UnicodeEngine for IcuUnicodeEngine {
        type SegmentationContext = IcuUnicodeSegmenters;

        fn char_properties(&self, ch: char) -> CharProperties {
            let props = parley_data::Properties::get(ch);
            let script = script_from_icu(props.script());
            CharProperties {
                script,
                bidi_class: BidiClass::from_icu4c_value(props.bidi_class().to_icu4c_value() as u8),
                bidi_bracket: bidi_bracket_from_icu(ch),
                is_regional_indicator: props.is_region_indicator(),
                is_extended_pictographic: icu_properties::props::ExtendedPictographic::for_char(ch),
                is_emoji_presentation: icu_properties::props::EmojiPresentation::for_char(ch),
            }
        }

        fn segmentation_context(&self) -> Self::SegmentationContext {
            IcuUnicodeSegmenters::new()
        }
    }

    type GraphemeIter<'s> = icu_segmenter::iterators::GraphemeClusterBreakIterator<
        'static,
        's,
        icu_segmenter::scaffold::Utf8,
    >;
    type WordIter<'s> = icu_segmenter::iterators::WordBreakIteratorWithWordType<
        'static,
        's,
        icu_segmenter::scaffold::Utf8,
    >;
    type LineIter<'s> =
        icu_segmenter::iterators::LineBreakIterator<'static, 's, icu_segmenter::scaffold::Utf8>;

    #[derive(Copy, Clone, Debug)]
    pub struct IcuUnicodeSegmenters {
        grapheme_segmenter: icu_segmenter::GraphemeClusterSegmenterBorrowed<'static>,
        word_segmenter: icu_segmenter::WordSegmenterBorrowed<'static>,
    }

    impl IcuUnicodeSegmenters {
        fn new() -> Self {
            Self {
                grapheme_segmenter: icu_segmenter::GraphemeClusterSegmenter::new(),
                word_segmenter: icu_segmenter::WordSegmenter::new_auto(
                    WordBreakInvariantOptions::default(),
                ),
            }
        }

        fn line_segmenter(
            &self,
            properties: TextAnalysisProperties,
        ) -> icu_segmenter::LineSegmenterBorrowed<'static> {
            let strictness = Some(match properties.line_break {
                LineBreak::Loose => LineBreakStrictness::Loose,
                LineBreak::Normal => LineBreakStrictness::Normal,
                LineBreak::Strict => LineBreakStrictness::Strict,
                LineBreak::Anywhere => LineBreakStrictness::Anywhere,
            });
            let word_option = Some(match properties.word_break {
                WordBreak::Normal => LineBreakWordOption::Normal,
                WordBreak::KeepAll => {
                    if properties.line_break != LineBreak::Anywhere {
                        // icu4x seems to prioritize break-all over anywhere even though
                        // the spec says otherwise
                        // <https://drafts.csswg.org/css-text-3/#valdef-line-break-anywhere>
                        LineBreakWordOption::KeepAll
                    } else {
                        LineBreakWordOption::Normal
                    }
                }
                WordBreak::BreakAll => LineBreakWordOption::BreakAll,
            });
            let lang = properties
                .language
                .as_ref()
                .and_then(|lang| LanguageIdentifier::try_from_str(lang.as_str()).ok());

            let mut options = IcuLineBreakOptions::default();
            options.strictness = strictness;
            options.word_option = word_option;
            options.content_locale = lang.as_ref();
            icu_segmenter::LineSegmenter::new_auto(options)
        }
    }

    impl UnicodeSegmentationContext for IcuUnicodeSegmenters {
        type Cursor<'s> = IcuUnicodeSegmentationState<'s>;

        fn cursor<'s>(
            &'s self,
            text: &'s str,
            properties: TextAnalysisProperties,
        ) -> Self::Cursor<'s> {
            IcuUnicodeSegmentationState {
                graphemes: self.grapheme_segmenter.segment_str(text),
                words: self.word_segmenter.segment_str(text).iter_with_word_type(),
                lines: self.line_segmenter(properties).segment_str(text),
            }
        }
    }

    #[derive(Debug)]
    pub struct IcuUnicodeSegmentationState<'s> {
        graphemes: GraphemeIter<'s>,
        words: WordIter<'s>,
        lines: LineIter<'s>,
    }

    impl<'s> UnicodeSegmentationCursor<'s> for IcuUnicodeSegmentationState<'s> {
        type Context = IcuUnicodeSegmenters;

        fn reset_text_boundaries(&mut self, context: &'s Self::Context, text: &'s str) {
            self.graphemes = context.grapheme_segmenter.segment_str(text);
            self.words = context
                .word_segmenter
                .segment_str(text)
                .iter_with_word_type();
        }

        fn reset_line_boundaries(
            &mut self,
            context: &'s Self::Context,
            text: &'s str,
            properties: TextAnalysisProperties,
        ) {
            self.lines = context.line_segmenter(properties).segment_str(text);
        }

        fn next_grapheme(&mut self) -> Option<usize> {
            self.graphemes.next()
        }

        fn next_word(&mut self) -> Option<(usize, WordKind)> {
            self.words.next().map(|(ix, kind)| {
                let kind = match kind {
                    icu_segmenter::options::WordType::Letter => WordKind::Letter,
                    icu_segmenter::options::WordType::Number => WordKind::Number,
                    _ => WordKind::Other,
                };
                (ix, kind)
            })
        }
        fn next_line(&mut self) -> Option<usize> {
            self.lines.next()
        }
    }

    fn bidi_bracket_from_icu(ch: char) -> Option<BidiBracket> {
        let bracket = BidiMirroringGlyph::for_char(ch);
        match bracket.paired_bracket_type {
            BidiPairedBracketType::Open => bracket.mirroring_glyph.map(BidiBracket::Open),
            BidiPairedBracketType::Close => bracket.mirroring_glyph.map(BidiBracket::Close),
            BidiPairedBracketType::None => None,
            _ => None,
        }
    }

    fn script_from_icu(script: IcuScript) -> Script {
        script
            .short_name()
            .as_bytes()
            .get(..4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(Script::from_bytes)
            .unwrap_or(Script::UNKNOWN)
    }
}
