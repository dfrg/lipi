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
    fn segment_events_with_emits_ordered_boundaries() {
        let text = "a\u{0301}";
        let p = TextAnalysisProperties::default();
        let analysis = analyze_with_element_props(text, &[(p, 1), (p, text.len() - 1)]);

        let segment_index = analysis
            .segments
            .iter()
            .position(|s| matches!(s, Segment::Text(_)))
            .unwrap();

        #[derive(Default)]
        struct RecordingSink {
            events: Vec<(usize, usize, usize, usize)>,
        }

        impl SegmentEventSink for RecordingSink {
            fn start_cluster(&mut self, cluster: &Cluster) {
                self.events.push((
                    0usize,
                    cluster.text_range().start,
                    cluster.text_range().end,
                    0usize,
                ));
            }

            fn end_cluster(&mut self) {
                self.events.push((1, 0, 0, 0));
            }

            fn element(&mut self, element: &Element) {
                self.events.push((2, element.handle.id as usize, 0, 0));
            }

            fn char_at(&mut self, ch: char, byte_index: usize) {
                self.events.push((3, byte_index, ch as usize, 0));
            }
        }

        let mut sink = RecordingSink::default();
        let Segment::Text(segment) = &analysis.segments[segment_index] else {
            panic!("expected text segment");
        };
        segment.events(text, &analysis, &mut sink);
        let events = sink.events;

        assert_eq!(events[0], (2, 0, 0, 0));
        assert_eq!(events[1], (0, 0, 3, 0));
        assert_eq!(events[2], (3, 0, 'a' as usize, 0));
        assert_eq!(events[3], (2, 1, 0, 0));
        assert_eq!(events[4], (3, 1, '\u{0301}' as usize, 0));
        assert_eq!(events[5], (1, 0, 0, 0));
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

        #[derive(Default)]
        struct RecordingSink {
            events: Vec<(usize, usize, usize, usize)>,
        }

        impl SegmentEventSink for RecordingSink {
            fn start_cluster(&mut self, cluster: &Cluster) {
                self.events.push((
                    0usize,
                    cluster.text_range().start,
                    cluster.text_range().end,
                    0usize,
                ));
            }

            fn end_cluster(&mut self) {
                self.events.push((1, 0, 0, 0));
            }

            fn element(&mut self, element: &Element) {
                self.events.push((2, element.handle.id as usize, 0, 0));
            }

            fn char_at(&mut self, ch: char, byte_index: usize) {
                self.events.push((3, byte_index, ch as usize, 0));
            }
        }

        let mut sink = RecordingSink::default();
        let Segment::Text(segment) = &analysis.segments[segment_index] else {
            panic!("expected text segment");
        };
        segment.events("ab", &analysis, &mut sink);
        let events = sink.events;

        assert_eq!(events[0], (2, 0, 0, 0));
        assert_eq!(events[1], (0, 0, 1, 0));
        assert_eq!(events[2], (3, 0, 'a' as usize, 0));
        assert_eq!(events[3], (1, 0, 0, 0));
        assert_eq!(events[4], (2, 1, 0, 0));
        assert_eq!(events[5], (2, 2, 0, 0));
        assert_eq!(events[6], (2, 3, 0, 0));
        assert_eq!(events[7], (0, 1, 2, 0));
        assert_eq!(events[8], (3, 1, 'b' as usize, 0));
        assert_eq!(events[9], (1, 0, 0, 0));
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
