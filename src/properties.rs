//! Extra properties and conversions.

// TODO: add this to parlance

/// Defines the "strictness" of line breaking.
///
/// Each variant has the same meaning with respect to the `line-break`
/// property values in the CSS Text spec. See the details in
/// <https://drafts.csswg.org/css-text-3/#line-break-property>.
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug)]
pub enum LineBreak {
    /// Breaks text using the least restrictive set of line-breaking rules.
    /// Typically used for short lines, such as in newspapers.
    /// <https://drafts.csswg.org/css-text-3/#valdef-line-break-loose>
    Loose,
    /// Breaks text using the most common set of line-breaking rules.
    /// <https://drafts.csswg.org/css-text-3/#valdef-line-break-normal>
    Normal,
    /// Breaks text using the most stringent set of line-breaking rules.
    /// <https://drafts.csswg.org/css-text-3/#valdef-line-break-strict>
    ///
    /// This is the default behaviour of the Unicode Line Breaking Algorithm,
    /// resolving class [CJ](https://www.unicode.org/reports/tr14/#CJ) to
    /// [NS](https://www.unicode.org/reports/tr14/#NS);
    /// see rule [LB1](https://www.unicode.org/reports/tr14/#LB1).
    #[default]
    Strict,
    /// Breaks text assuming there is a soft wrap opportunity around every
    /// typographic character unit, disregarding any prohibition against line
    /// breaks. See more details in
    /// <https://drafts.csswg.org/css-text-3/#valdef-line-break-anywhere>.
    Anywhere,
}

use core::convert::TryInto;
use icu_locale_core::LanguageIdentifier;
use icu_properties::props::{NamedEnumeratedProperty, Script as IcuScript};
use icu_segmenter::options::{
    LineBreakOptions as IcuLineBreakOptions, LineBreakStrictness as IcuLineBreak,
    LineBreakWordOption as IcuWordBreak,
};
use parlance::{Language, Script, WordBreak};

pub(crate) fn script_from_icu(script: IcuScript) -> Script {
    script
        .short_name()
        .as_bytes()
        .get(..4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(|bytes| Script::from_bytes(bytes))
        .unwrap_or(Script::UNKNOWN)
}

#[derive(Default)]
pub(crate) struct LineBreakOptions {
    strictness: Option<IcuLineBreak>,
    word_option: Option<IcuWordBreak>,
    lang: Option<LanguageIdentifier>,
}

impl LineBreakOptions {
    pub(crate) fn new(
        line_break: LineBreak,
        word_break: WordBreak,
        lang: Option<Language>,
    ) -> Self {
        use icu_segmenter::options::{LineBreakStrictness, LineBreakWordOption};
        let mut options = Self::default();
        options.strictness = Some(match line_break {
            LineBreak::Loose => LineBreakStrictness::Loose,
            LineBreak::Normal => LineBreakStrictness::Normal,
            LineBreak::Strict => LineBreakStrictness::Strict,
            LineBreak::Anywhere => LineBreakStrictness::Anywhere,
        });
        options.word_option = Some(match word_break {
            WordBreak::Normal => LineBreakWordOption::Normal,
            WordBreak::KeepAll => {
                if line_break != LineBreak::Anywhere {
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
        options.lang = lang
            .as_ref()
            .and_then(|lang| LanguageIdentifier::try_from_str(lang.as_str()).ok());
        options
    }

    pub(crate) fn get(&self) -> IcuLineBreakOptions<'_> {
        let mut options = IcuLineBreakOptions::default();
        options.strictness = self.strictness;
        options.word_option = self.word_option;
        options.content_locale = self.lang.as_ref();
        options
    }
}
