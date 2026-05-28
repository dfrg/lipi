//! Text analysis.

mod analysis;
mod analyzer;
mod bidi;
mod element;
mod properties;
pub mod unicode;

use crate::element::ElementHandle;
use crate::{Language, Script};

pub use analysis::{
    Cluster, ClusterAnalysis, ClusterAttributes, ClusterContent, ClusterRangeIter, Paragraph,
    Segment, SegmentEvent, SegmentEventSink, TextAnalysis, TextSegment, WordKind,
};
pub use analyzer::TextAnalyzer;
pub use bidi::BidiLevel;
pub use element::{BidiControl, SourceElement, SourceElementKind};
pub use parlance::{BidiDirection, BidiOverride, WordBreak};
pub use properties::LineBreak;

/// Properties that control text analysis.
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug)]
pub struct TextAnalysisProperties {
    /// The line-break property.
    pub line_break: LineBreak,
    /// The word-break property.
    pub word_break: WordBreak,
    /// The language.
    pub language: Option<Language>,
}

/// Interface for mapping an element handle to text analysis properties.
pub trait TextAnalysisPropertiesProvider {
    /// Returns the properties for the given element handle.
    fn text_analysis_properties(&mut self, handle: &ElementHandle) -> TextAnalysisProperties;
}

fn is_paragraph_separator(ch: char) -> bool {
    matches!(
        ch,
        '\n' | '\r' | '\u{2029}' | '\u{2028}' | '\u{000B}' | '\u{000C}'
    )
}

fn is_real_script(script: Script) -> bool {
    script != Script::COMMON && script != Script::INHERITED && script != Script::UNKNOWN
}

#[cfg(test)]
mod unicode_engine_tests {
    use super::*;
    use crate::element::ElementHandle;
    use crate::text::unicode::{CharProperties, UnicodeEngine, UnicodeSegmentationContext};

    #[derive(Copy, Clone, Default)]
    struct TestEngine;

    #[derive(Copy, Clone, Default)]
    struct TestSegmenters;

    struct TestSegmentationState<'s> {
        text: &'s str,
        next_grapheme_local: usize,
        word_step: u8,
        line_step: u8,
    }

    impl UnicodeEngine for TestEngine {
        type SegmentationContext = TestSegmenters;

        fn char_properties(&self, ch: char) -> CharProperties {
            let bidi_class = if ch.is_ascii_alphabetic() {
                bidi::BidiClass::LEFT_TO_RIGHT
            } else {
                bidi::BidiClass::OTHER_NEUTRAL
            };
            CharProperties {
                script: if ch.is_ascii_alphabetic() {
                    Script::from_bytes(*b"Latn")
                } else {
                    Script::COMMON
                },
                bidi_class,
                bidi_bracket: None,
                is_regional_indicator: false,
                is_extended_pictographic: false,
                is_emoji_presentation: false,
            }
        }

        fn segmentation_context(&self) -> Self::SegmentationContext {
            TestSegmenters
        }
    }

    impl UnicodeSegmentationContext for TestSegmenters {
        type Cursor<'s> = TestSegmentationState<'s>;

        fn cursor<'s>(
            &'s self,
            text: &'s str,
            _properties: TextAnalysisProperties,
        ) -> Self::Cursor<'s> {
            TestSegmentationState {
                text,
                next_grapheme_local: 0,
                word_step: 0,
                line_step: 0,
            }
        }
    }

    impl<'s> unicode::UnicodeSegmentationCursor<'s> for TestSegmentationState<'s> {
        type Context = TestSegmenters;

        fn reset_text_boundaries(&mut self, _context: &'s Self::Context, text: &'s str) {
            self.text = text;
            self.next_grapheme_local = 0;
            self.word_step = 0;
        }

        fn reset_line_boundaries(
            &mut self,
            _context: &'s Self::Context,
            text: &'s str,
            _properties: TextAnalysisProperties,
        ) {
            self.text = text;
            self.line_step = 0;
        }

        fn next_grapheme(&mut self) -> Option<usize> {
            if self.next_grapheme_local > self.text.len() {
                return None;
            }
            let out = self.next_grapheme_local;
            if self.next_grapheme_local == self.text.len() {
                self.next_grapheme_local = self.text.len() + 1;
                return Some(out);
            }
            let mut next = self.next_grapheme_local + 1;
            while next <= self.text.len() && !self.text.is_char_boundary(next) {
                next += 1;
            }
            self.next_grapheme_local = next;
            Some(out)
        }

        fn next_word(&mut self) -> Option<(usize, WordKind)> {
            let out = match self.word_step {
                0 => Some((0, WordKind::Other)),
                1 => Some((self.text.len(), WordKind::Letter)),
                _ => None,
            };
            self.word_step = self.word_step.saturating_add(1);
            out
        }

        fn next_line(&mut self) -> Option<usize> {
            let out = match self.line_step {
                0 => Some(0),
                1 => Some(self.text.len()),
                _ => None,
            };
            self.line_step = self.line_step.saturating_add(1);
            out
        }
    }

    #[test]
    fn analyze_with_custom_engine_works() {
        let text = "ab";
        struct PropProvider(TextAnalysisProperties);

        impl TextAnalysisPropertiesProvider for PropProvider {
            fn text_analysis_properties(
                &mut self,
                _handle: &ElementHandle,
            ) -> TextAnalysisProperties {
                self.0
            }
        }

        let mut props = PropProvider(TextAnalysisProperties::default());
        let mut analysis = TextAnalysis::default();
        let mut analyzer = TextAnalyzer::default();
        analyzer
            .analyze_with_unicode_engine(
                text,
                BidiDirection::Auto,
                &TestEngine,
                &mut props,
                [SourceElement {
                    handle: ElementHandle::default(),
                    kind: SourceElementKind::Text(text.len() as u32),
                }]
                .iter()
                .copied(),
                &mut analysis,
            )
            .unwrap();

        assert_eq!(analysis.clusters.len(), 2);
        assert_eq!(analysis.segments.len(), 1);
        assert_eq!(analysis.paragraphs.len(), 1);
        assert_eq!(analysis.paragraphs[0].segments(), 0..1);
    }
}

#[cfg(all(test, feature = "icu"))]
mod tests {
    use super::*;
    use crate::element::*;

    fn analyze(text: &str, props: Option<TextAnalysisProperties>) -> TextAnalysis {
        analyze_with_base_direction(text, BidiDirection::Auto, props)
    }

    fn analyze_with_base_direction(
        text: &str,
        base_direction: BidiDirection,
        props: Option<TextAnalysisProperties>,
    ) -> TextAnalysis {
        let mut props = props.unwrap_or_default();
        let mut a = TextAnalysis::default();
        TextAnalyzer::default()
            .analyze(
                text,
                base_direction,
                &mut props,
                [SourceElement {
                    handle: ElementHandle::default(),
                    kind: SourceElementKind::Text(text.len() as u32),
                }]
                .iter()
                .copied(),
                &mut a,
            )
            .unwrap();
        a
    }

    fn analyze_with_element_props(
        text: &str,
        element_props: &[(TextAnalysisProperties, usize)],
    ) -> TextAnalysis {
        struct PropSet<'a>(&'a [TextAnalysisProperties]);

        impl TextAnalysisPropertiesProvider for PropSet<'_> {
            fn text_analysis_properties(
                &mut self,
                handle: &ElementHandle,
            ) -> TextAnalysisProperties {
                self.0[handle.id as usize]
            }
        }

        let props = element_props.iter().map(|(p, _)| *p).collect::<Vec<_>>();
        let mut prop_set = PropSet(&props);
        let mut a = TextAnalysis::default();
        TextAnalyzer::default()
            .analyze(
                text,
                BidiDirection::Auto,
                &mut prop_set,
                element_props
                    .iter()
                    .enumerate()
                    .map(|(i, (_, len))| SourceElement {
                        handle: ElementHandle {
                            id: i as u64,
                            context_id: 0,
                        },
                        kind: SourceElementKind::Text(*len as u32),
                    }),
                &mut a,
            )
            .unwrap();
        a
    }

    fn analyze_with_elements(
        text: &str,
        elements: &[(TextAnalysisProperties, SourceElementKind)],
    ) -> TextAnalysis {
        struct PropSet<'a>(&'a [TextAnalysisProperties]);

        impl TextAnalysisPropertiesProvider for PropSet<'_> {
            fn text_analysis_properties(
                &mut self,
                handle: &ElementHandle,
            ) -> TextAnalysisProperties {
                self.0[handle.id as usize]
            }
        }

        let props = elements.iter().map(|(p, _)| *p).collect::<Vec<_>>();
        let mut prop_set = PropSet(&props);
        let mut a = TextAnalysis::default();
        TextAnalyzer::default()
            .analyze(
                text,
                BidiDirection::Auto,
                &mut prop_set,
                elements
                    .iter()
                    .enumerate()
                    .map(|(i, (_, kind))| SourceElement {
                        handle: ElementHandle {
                            id: i as u64,
                            context_id: 0,
                        },
                        kind: *kind,
                    }),
                &mut a,
            )
            .unwrap();
        a
    }

    impl TextAnalysisPropertiesProvider for TextAnalysisProperties {
        fn text_analysis_properties(&mut self, _handle: &ElementHandle) -> TextAnalysisProperties {
            *self
        }
    }

    #[test]
    fn paragraph_levels_and_segment_ranges() {
        // First paragraph is LTR, second paragraph is RTL.
        let text = "abc\nאבג";
        let analysis = analyze(text, None);

        assert_eq!(analysis.paragraphs.len(), 2);
        assert_eq!(analysis.paragraphs[0].bidi_level, 0);
        assert_eq!(analysis.paragraphs[1].bidi_level, 1);

        // Paragraph segment ranges should partition the segment stream.
        assert_eq!(analysis.paragraphs[0].segments().start, 0);
        assert_eq!(
            analysis.paragraphs[0].segments().end,
            analysis.paragraphs[1].segments().start
        );
        assert_eq!(
            analysis.paragraphs[1].segments().end,
            analysis.segments.len()
        );
        assert!(analysis.paragraphs.iter().all(|p| !p.segments().is_empty()));
    }

    #[test]
    fn paragraph_boundaries_split_segment_stream() {
        let text = "abc\ndef";
        let analysis = analyze(text, None);

        assert_eq!(analysis.paragraphs.len(), 2);
        assert!(analysis.segments.len() >= 2);

        let p0 = analysis.paragraphs[0].segments();
        let p1 = analysis.paragraphs[1].segments();
        assert_eq!(p0.start, 0);
        assert_eq!(p0.end, p1.start);
        assert_eq!(p1.end, analysis.segments.len());

        // Ensure no segment index belongs to more than one paragraph.
        assert!(p0.end <= p1.start);
    }

    #[test]
    fn explicit_base_direction_sets_paragraph_level() {
        // Digits provide no strong paragraph direction, so the explicit base level should win.
        let text = "123";
        let ltr = analyze_with_base_direction(text, BidiDirection::Ltr, None);
        let rtl = analyze_with_base_direction(text, BidiDirection::Rtl, None);

        assert_eq!(ltr.paragraphs.len(), 1);
        assert_eq!(rtl.paragraphs.len(), 1);
        assert_eq!(ltr.paragraphs[0].bidi_level, 0);
        assert_eq!(rtl.paragraphs[0].bidi_level, 1);
    }

    #[test]
    fn word_break_break_all_enables_mid_word_breaks() {
        let text = "ab";
        let normal = analyze(
            text,
            Some(TextAnalysisProperties {
                line_break: LineBreak::Strict,
                word_break: WordBreak::Normal,
                language: None,
            }),
        );
        let break_all = analyze(
            text,
            Some(TextAnalysisProperties {
                line_break: LineBreak::Strict,
                word_break: WordBreak::BreakAll,
                language: None,
            }),
        );

        assert_eq!(normal.clusters.len(), 2);
        assert_eq!(break_all.clusters.len(), 2);
        assert!(!normal.clusters.get(0).unwrap().can_break_line_after());
        assert!(break_all.clusters.get(0).unwrap().can_break_line_after());
    }

    #[test]
    fn line_break_anywhere_overrides_keep_all() {
        let text = "ab";
        let keep_all_strict = analyze(
            text,
            Some(TextAnalysisProperties {
                line_break: LineBreak::Strict,
                word_break: WordBreak::KeepAll,
                language: None,
            }),
        );
        let keep_all_anywhere = analyze(
            text,
            Some(TextAnalysisProperties {
                line_break: LineBreak::Anywhere,
                word_break: WordBreak::KeepAll,
                language: None,
            }),
        );

        assert_eq!(keep_all_strict.clusters.len(), 2);
        assert_eq!(keep_all_anywhere.clusters.len(), 2);
        assert!(!keep_all_strict
            .clusters
            .get(0)
            .unwrap()
            .can_break_line_after());
        assert!(keep_all_anywhere
            .clusters
            .get(0)
            .unwrap()
            .can_break_line_after());
    }

    #[test]
    fn line_break_behavior_changes_when_properties_change() {
        let text = "abcd";
        let normal = TextAnalysisProperties {
            line_break: LineBreak::Strict,
            word_break: WordBreak::Normal,
            language: None,
        };
        let break_all = TextAnalysisProperties {
            line_break: LineBreak::Strict,
            word_break: WordBreak::BreakAll,
            language: None,
        };

        let all_normal = analyze_with_element_props(text, &[(normal, 2), (normal, 2)]);
        let second_break_all = analyze_with_element_props(text, &[(normal, 2), (break_all, 2)]);

        assert_eq!(all_normal.clusters.len(), 4);
        assert_eq!(second_break_all.clusters.len(), 4);

        // The first half is unchanged, so the first cluster should match.
        assert_eq!(
            all_normal.clusters.get(0).unwrap().can_break_line_after(),
            second_break_all
                .clusters
                .get(0)
                .unwrap()
                .can_break_line_after()
        );

        // The second half switches to BreakAll, so "c" should become breakable.
        assert!(!all_normal.clusters.get(2).unwrap().can_break_line_after());
        assert!(second_break_all
            .clusters
            .get(2)
            .unwrap()
            .can_break_line_after());
    }

    #[test]
    fn detects_emoji_and_symbol_content() {
        let emoji = analyze("🙂", None);
        let symbol = analyze("❤", None);

        assert_eq!(emoji.clusters.len(), 1);
        assert_eq!(symbol.clusters.len(), 1);
        assert_eq!(
            emoji.clusters.get(0).unwrap().content(),
            ClusterContent::Emoji
        );
        assert_eq!(
            symbol.clusters.get(0).unwrap().content(),
            ClusterContent::Symbol
        );
    }

    #[test]
    fn variation_selector_changes_symbol_presentation() {
        let text_default = analyze("❤", None);
        let text_presentation = analyze("❤\u{FE0E}", None);
        let emoji_presentation = analyze("❤\u{FE0F}", None);

        assert_eq!(text_default.clusters.len(), 1);
        assert_eq!(text_presentation.clusters.len(), 1);
        assert_eq!(emoji_presentation.clusters.len(), 1);

        assert_eq!(
            text_default.clusters.get(0).unwrap().content(),
            ClusterContent::Symbol
        );
        assert_eq!(
            text_presentation.clusters.get(0).unwrap().content(),
            ClusterContent::Symbol
        );
        assert_eq!(
            emoji_presentation.clusters.get(0).unwrap().content(),
            ClusterContent::Emoji
        );
    }

    #[test]
    fn classifies_whitespace_content_types() {
        let analysis = analyze(" \t\u{00A0}\u{2003}", None);

        assert_eq!(analysis.clusters.len(), 4);
        assert_eq!(
            analysis.clusters.get(0).unwrap().content(),
            ClusterContent::Space
        );
        assert_eq!(
            analysis.clusters.get(1).unwrap().content(),
            ClusterContent::Tab
        );
        assert_eq!(
            analysis.clusters.get(2).unwrap().content(),
            ClusterContent::NoBreakSpace
        );
        assert_eq!(
            analysis.clusters.get(3).unwrap().content(),
            ClusterContent::OtherWhitespace
        );
    }

    #[test]
    fn paragraph_separator_variants_split_paragraphs() {
        let crlf = analyze("a\r\nb", None);
        let line_sep = analyze("a\u{2028}b", None);
        let para_sep = analyze("a\u{2029}b", None);

        // CRLF is a single grapheme cluster, and should still be treated as
        // a paragraph separator.
        assert!(
            crlf.clusters
                .iter()
                .any(|c| c.text_range() == (1..3)
                    && c.content() == ClusterContent::ParagraphSeparator)
        );
        assert_eq!(crlf.paragraphs.len(), 2);

        // Unicode line/paragraph separators should split into two paragraphs.
        assert_eq!(line_sep.paragraphs.len(), 2);
        assert_eq!(para_sep.paragraphs.len(), 2);
    }

    #[test]
    fn detects_regional_indicator_content() {
        let indicator = analyze("🇦", None);
        let flag = analyze("🇫🇷", None);

        assert_eq!(indicator.clusters.len(), 1);
        assert_eq!(flag.clusters.len(), 1);
        assert_eq!(
            indicator.clusters.get(0).unwrap().content(),
            ClusterContent::RegionalIndicator
        );
        assert_eq!(
            flag.clusters.get(0).unwrap().content(),
            ClusterContent::RegionalIndicator
        );
    }

    #[test]
    fn classifies_word_kind_for_letters_numbers_and_other() {
        let letters = analyze("abc", None);
        let numbers = analyze("123", None);
        let other = analyze("!", None);

        assert_eq!(letters.clusters.len(), 3);
        assert_eq!(numbers.clusters.len(), 3);
        assert_eq!(other.clusters.len(), 1);

        assert_eq!(
            letters.clusters.get(2).unwrap().word_kind(),
            Some(WordKind::Letter)
        );
        assert_eq!(
            numbers.clusters.get(2).unwrap().word_kind(),
            Some(WordKind::Number)
        );
        assert_eq!(
            other.clusters.get(0).unwrap().word_kind(),
            Some(WordKind::Other)
        );
    }

    #[test]
    fn non_text_elements_split_segments_as_expected() {
        let p = TextAnalysisProperties::default();

        let with_object = analyze_with_elements(
            "ab",
            &[
                (p, SourceElementKind::Text(1)),
                (p, SourceElementKind::Object(BidiDirection::Auto)),
                (p, SourceElementKind::Text(1)),
            ],
        );
        assert_eq!(with_object.segments.len(), 3);
        assert!(matches!(with_object.segments[0], Segment::Text(_)));
        assert!(matches!(with_object.segments[1], Segment::Object(_, _)));
        assert!(matches!(with_object.segments[2], Segment::Text(_)));

        let with_break = analyze_with_elements(
            "ab",
            &[
                (p, SourceElementKind::Text(1)),
                (p, SourceElementKind::SegmentationBreak),
                (p, SourceElementKind::Text(1)),
            ],
        );
        assert_eq!(with_break.segments.len(), 2);
        assert!(matches!(with_break.segments[0], Segment::Text(_)));
        assert!(matches!(with_break.segments[1], Segment::Text(_)));
    }

    #[test]
    fn segment_and_paragraph_ranges_form_partitions() {
        let p = TextAnalysisProperties::default();
        let analysis = analyze_with_elements(
            "ab\ncd",
            &[
                (p, SourceElementKind::Text(2)),
                (p, SourceElementKind::Object(BidiDirection::Auto)),
                (p, SourceElementKind::Text(3)),
            ],
        );

        // Paragraph ranges must be a contiguous partition of the segment list.
        let mut segment_cursor = 0;
        for paragraph in &analysis.paragraphs {
            let segments = paragraph.segments();
            assert_eq!(segments.start, segment_cursor);
            assert!(segments.start <= segments.end);
            assert!(segments.end <= analysis.segments.len());
            segment_cursor = segments.end;
        }
        assert_eq!(segment_cursor, analysis.segments.len());

        // Every object segment index should be covered by exactly one paragraph
        // range, so objects always belong to their containing paragraph.
        for (segment_idx, segment) in analysis.segments.iter().enumerate() {
            if matches!(segment, Segment::Object(_, _)) {
                let containing = analysis
                    .paragraphs
                    .iter()
                    .filter(|p| p.segments().contains(&segment_idx))
                    .count();
                assert_eq!(containing, 1);
            }
        }

        // Text segment cluster ranges should also be contiguous and cover all
        // clusters exactly once.
        let mut cluster_cursor = 0;
        for segment in &analysis.segments {
            if let Segment::Text(text) = segment {
                let clusters = text.clusters();
                assert!(clusters.start < clusters.end);
                assert_eq!(clusters.start, cluster_cursor);
                cluster_cursor = clusters.end;
            }
        }
        assert_eq!(cluster_cursor, analysis.clusters.len());
    }

    #[test]
    fn auto_base_direction_uses_first_strong_character() {
        let hebrew_first = "אבג abc";
        let latin_first = "abc אבג";

        let auto_hebrew_first = analyze(hebrew_first, None);
        let auto_latin_first = analyze(latin_first, None);
        let forced_ltr = analyze_with_base_direction(hebrew_first, BidiDirection::Ltr, None);
        let forced_rtl = analyze_with_base_direction(latin_first, BidiDirection::Rtl, None);

        assert_eq!(auto_hebrew_first.paragraphs.len(), 1);
        assert_eq!(auto_latin_first.paragraphs.len(), 1);
        assert_eq!(auto_hebrew_first.paragraphs[0].bidi_level, 1);
        assert_eq!(auto_latin_first.paragraphs[0].bidi_level, 0);
        assert_eq!(forced_ltr.paragraphs[0].bidi_level, 0);
        assert_eq!(forced_rtl.paragraphs[0].bidi_level, 1);
    }

    #[test]
    fn bidi_controls_do_not_create_phantom_text_clusters() {
        let p = TextAnalysisProperties::default();
        let with_isolate = analyze_with_elements(
            "ab",
            &[
                (p, SourceElementKind::Text(1)),
                (
                    p,
                    SourceElementKind::BidiControl(BidiControl::PushIsolate(BidiDirection::Rtl)),
                ),
                (p, SourceElementKind::Text(1)),
                (p, SourceElementKind::BidiControl(BidiControl::PopIsolate)),
            ],
        );
        let with_override = analyze_with_elements(
            "ab",
            &[
                (p, SourceElementKind::Text(1)),
                (
                    p,
                    SourceElementKind::BidiControl(BidiControl::PushOverride(BidiOverride::Rtl)),
                ),
                (p, SourceElementKind::Text(1)),
                (p, SourceElementKind::BidiControl(BidiControl::PopOverride)),
            ],
        );

        for analysis in [&with_isolate, &with_override] {
            assert_eq!(analysis.clusters.len(), 2);
            let text_cluster_count = analysis
                .segments
                .iter()
                .filter_map(|segment| match segment {
                    Segment::Text(text) => Some(text.clusters().len()),
                    Segment::Object(_, _) => None,
                })
                .sum::<usize>();
            assert_eq!(text_cluster_count, analysis.clusters.len());
        }
    }

    #[test]
    fn language_change_splits_text_segments() {
        let en_props = TextAnalysisProperties {
            language: Some(Language::parse_prefix("en").unwrap().0),
            ..Default::default()
        };
        let fr_props = TextAnalysisProperties {
            language: Some(Language::parse_prefix("fr").unwrap().0),
            ..Default::default()
        };

        let same_language = analyze_with_element_props("abcd", &[(en_props, 2), (en_props, 2)]);
        let mixed_language = analyze_with_element_props("abcd", &[(en_props, 2), (fr_props, 2)]);

        let same_language_text_segments = same_language
            .segments
            .iter()
            .filter(|segment| matches!(segment, Segment::Text(_)))
            .count();
        let mixed_language_text_segments = mixed_language
            .segments
            .iter()
            .filter(|segment| matches!(segment, Segment::Text(_)))
            .count();

        assert_eq!(same_language_text_segments, 1);
        assert_eq!(mixed_language_text_segments, 2);
    }

    #[test]
    fn segment_events_emits_ordered_boundaries() {
        let text = "a\u{0301}";
        let p = TextAnalysisProperties::default();
        let analysis = analyze_with_element_props(text, &[(p, 1), (p, text.len() - 1)]);

        let segment_index = analysis
            .segments
            .iter()
            .position(|s| matches!(s, Segment::Text(_)))
            .unwrap();
        let events = analysis
            .segment_events(text, segment_index)
            .unwrap()
            .collect::<Vec<_>>();

        assert!(matches!(
            events[0],
            SegmentEvent::Element(element) if element.handle.id == 0
        ));
        assert!(matches!(events[1], SegmentEvent::StartCluster(_)));
        assert!(matches!(events[2], SegmentEvent::Char('a', 0)));
        assert!(matches!(
            events[3],
            SegmentEvent::Element(element) if element.handle.id == 1
        ));
        assert!(matches!(events[4], SegmentEvent::Char('\u{0301}', 1)));
        assert!(matches!(events[5], SegmentEvent::EndCluster));
    }

    #[test]
    fn segment_events_emit_non_text_elements() {
        let p = TextAnalysisProperties::default();
        let analysis = analyze_with_elements(
            "ab",
            &[
                (p, SourceElementKind::Text(1)),
                (p, SourceElementKind::Marker),
                (p, SourceElementKind::StartSpan),
                (p, SourceElementKind::Text(1)),
            ],
        );

        let segment_index = analysis
            .segments
            .iter()
            .position(|s| matches!(s, Segment::Text(_)))
            .unwrap();
        let events = analysis
            .segment_events("ab", segment_index)
            .unwrap()
            .collect::<Vec<_>>();

        assert!(matches!(events[0], SegmentEvent::Element(element) if element.handle.id == 0));
        assert!(matches!(events[1], SegmentEvent::StartCluster(_)));
        assert!(matches!(events[2], SegmentEvent::Char('a', 0)));
        assert!(matches!(events[3], SegmentEvent::EndCluster));
        assert!(matches!(events[4], SegmentEvent::Element(element) if element.handle.id == 1));
        assert!(matches!(events[5], SegmentEvent::Element(element) if element.handle.id == 2));
        assert!(matches!(events[6], SegmentEvent::Element(element) if element.handle.id == 3));
        assert!(matches!(events[7], SegmentEvent::StartCluster(_)));
        assert!(matches!(events[8], SegmentEvent::Char('b', 1)));
        assert!(matches!(events[9], SegmentEvent::EndCluster));
    }

    #[test]
    fn segment_events_lowered_matches_default_iterator() {
        let p = TextAnalysisProperties::default();
        let analysis = analyze_with_elements(
            "a\u{0301}bc",
            &[
                (p, SourceElementKind::Text(1)),
                (p, SourceElementKind::Marker),
                (p, SourceElementKind::Text(2)),
                (p, SourceElementKind::StartSpan),
                (p, SourceElementKind::Text(1)),
            ],
        );

        let segment_index = analysis
            .segments
            .iter()
            .position(|s| matches!(s, Segment::Text(_)))
            .unwrap();

        let encode = |events: Vec<SegmentEvent<'_>>| {
            events
                .into_iter()
                .map(|event| match event {
                    SegmentEvent::StartCluster(cluster) => (
                        0usize,
                        cluster.text_range().start,
                        cluster.text_range().end,
                        0usize,
                    ),
                    SegmentEvent::EndCluster => (1, 0, 0, 0),
                    SegmentEvent::Element(element) => (2, element.handle.id as usize, 0, 0),
                    SegmentEvent::Char(ch, byte_index) => (3, byte_index, ch as usize, 0),
                })
                .collect::<Vec<_>>()
        };

        let default_events = encode(
            analysis
                .segment_events("a\u{0301}bc", segment_index)
                .unwrap()
                .collect(),
        );
        let lowered_events = encode(
            analysis
                .segment_events2_lowered("a\u{0301}bc", segment_index)
                .collect(),
        );

        assert_eq!(lowered_events, default_events);
    }

    #[test]
    fn script_backprop_assigns_following_real_script() {
        let en_props = TextAnalysisProperties {
            language: Some(Language::parse_prefix("en").unwrap().0),
            ..Default::default()
        };
        let fr_props = TextAnalysisProperties {
            language: Some(Language::parse_prefix("fr").unwrap().0),
            ..Default::default()
        };

        // Keep segments split by language so the leading Common-script space
        // cannot be resolved through merge_scripts and must be back-propagated.
        let analysis = analyze_with_element_props(" abc", &[(en_props, 1), (fr_props, 3)]);
        let text_segments = analysis
            .segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Text(text) => Some(text),
                Segment::Object(_, _) => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(text_segments.len(), 2);
        assert_eq!(text_segments[0].script, text_segments[1].script);
        assert_ne!(text_segments[0].script, Script::COMMON);
    }

    #[test]
    fn object_direction_influences_object_segment_level() {
        let p = TextAnalysisProperties::default();

        let auto_object = analyze_with_elements(
            "a",
            &[
                (p, SourceElementKind::Text(1)),
                (p, SourceElementKind::Object(BidiDirection::Auto)),
            ],
        );
        let ltr_object = analyze_with_elements(
            "a",
            &[
                (p, SourceElementKind::Text(1)),
                (p, SourceElementKind::Object(BidiDirection::Ltr)),
            ],
        );
        let rtl_object = analyze_with_elements(
            "a",
            &[
                (p, SourceElementKind::Text(1)),
                (p, SourceElementKind::Object(BidiDirection::Rtl)),
            ],
        );

        let auto_level = auto_object
            .segments
            .iter()
            .find_map(|segment| match segment {
                Segment::Object(level, _) => Some(*level),
                Segment::Text(_) => None,
            })
            .expect("expected object segment for auto object");
        let ltr_level = ltr_object
            .segments
            .iter()
            .find_map(|segment| match segment {
                Segment::Object(level, _) => Some(*level),
                Segment::Text(_) => None,
            })
            .expect("expected object segment for ltr object");
        let rtl_level = rtl_object
            .segments
            .iter()
            .find_map(|segment| match segment {
                Segment::Object(level, _) => Some(*level),
                Segment::Text(_) => None,
            })
            .expect("expected object segment for rtl object");

        assert_eq!(ltr_level & 1, 0);
        assert_eq!(rtl_level & 1, 1);
        assert_eq!(auto_level, ltr_level);
    }

    #[test]
    fn object_only_elements_emit_object_segments() {
        let p = TextAnalysisProperties::default();
        let analysis = analyze_with_elements(
            "",
            &[
                (p, SourceElementKind::Object(BidiDirection::Ltr)),
                (p, SourceElementKind::Object(BidiDirection::Rtl)),
            ],
        );

        assert!(analysis.clusters.is_empty());
        assert_eq!(analysis.segments.len(), 2);
        assert!(matches!(analysis.segments[0], Segment::Object(_, _)));
        assert!(matches!(analysis.segments[1], Segment::Object(_, _)));
        assert_eq!(analysis.paragraphs.len(), 1);
    }

    #[test]
    fn empty_input_produces_empty_analysis() {
        let analysis = analyze("", None);

        assert!(analysis.clusters.is_empty());
        assert!(analysis.segments.is_empty());
        assert!(analysis.paragraphs.is_empty());
    }

    #[test]
    fn control_only_elements_do_not_create_text_clusters() {
        let p = TextAnalysisProperties::default();
        let analysis = analyze_with_elements(
            "",
            &[
                (
                    p,
                    SourceElementKind::BidiControl(BidiControl::PushIsolate(BidiDirection::Rtl)),
                ),
                (p, SourceElementKind::BidiControl(BidiControl::PopIsolate)),
            ],
        );

        assert!(analysis.clusters.is_empty());
        assert!(analysis
            .segments
            .iter()
            .all(|segment| !matches!(segment, Segment::Text(_))));
        assert_eq!(analysis.paragraphs.len(), 1);
    }

    fn dump_analysis(text: &str, analysis: &TextAnalysis) {
        println!("text_segments = {:?}", analysis.segments);
        // for cluster in analysis.clusters.iter() {
        //     dump_cluster(text, &cluster);
        // }
        println!("");
        for ss in &analysis.segments {
            match ss {
                Segment::Object(level, handle) => {
                    println!("[object {} #{}]", *level, handle.0);
                }
                Segment::Text(t) => {
                    println!(
                        "[{} {:?} {}] {}",
                        t.script,
                        t.language,
                        t.bidi_level,
                        &text[analysis.clusters.text_range(t.clusters()).unwrap()]
                    );
                    for cluster in analysis.clusters.iter_range(t.clusters()) {
                        dump_cluster(text, &cluster);
                    }
                }
            }
        }
    }
}

#[allow(unused)]
fn dump_cluster(text: &str, cluster: &Cluster) {
    let cluster_text = &text[cluster.text_range()];
    let content = match cluster.content() {
        ClusterContent::Text => ' ',
        ClusterContent::Emoji => 'E',
        ClusterContent::Symbol => 'T',
        ClusterContent::ParagraphSeparator => 'N',
        ClusterContent::RegionalIndicator => 'r',
        ClusterContent::Space => 's',
        ClusterContent::Tab => 't',
        ClusterContent::NoBreakSpace => 'n',
        ClusterContent::OtherWhitespace => 'o',
    };
    let line = if cluster.can_break_line_after() {
        'L'
    } else {
        '_'
    };
    let word = match cluster.word_kind() {
        Some(WordKind::Letter) => 'l',
        Some(WordKind::Number) => 'n',
        Some(WordKind::Other) => 'o',
        None => '_',
    };
    let rtl = if cluster.is_rtl() { '<' } else { ' ' };
    let count = cluster_text.chars().count();
    println!(
        "[{content} {line} {word} {rtl}]:     {cluster_text:?} ({cluster_text}) ({count} chars)"
    );
}
