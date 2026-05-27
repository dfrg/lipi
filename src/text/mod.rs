//! Text analysis.

mod analysis;
mod analyzer;
mod bidi;
mod cluster;
mod element;
mod properties;

use crate::element::ElementHandle;
use crate::{Language, Script};
use core::ops::Range;
use properties::LineBreakOptions;

pub use analysis::{ClusterAnalysis, Paragraph, Segment, TextAnalysis, TextSegment};
pub use analyzer::TextAnalyzer;
pub use bidi::BidiLevel;
pub use cluster::{Cluster, ClusterAttributes, ClusterContent, WordKind};
pub use element::{BidiControl, SourceElement, SourceElementKind};
pub use parlance::{BidiDirection, BidiOverride, WordBreak};
pub use properties::LineBreak;

#[derive(Clone, PartialEq, Eq, Debug)]
struct PendingCluster {
    attrs: ClusterAttributes,
    range: Range<usize>,
    base_char: char,
    script: Script,
    lang: Option<Language>,
}

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

impl TextAnalysisProperties {
    fn line_break_options(&self) -> LineBreakOptions {
        LineBreakOptions::new(self.line_break, self.word_break, self.language)
    }
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
        assert_eq!(analysis.paragraphs[0].level, 0);
        assert_eq!(analysis.paragraphs[1].level, 1);

        // Paragraph segment ranges should partition the segment stream.
        assert_eq!(analysis.paragraphs[0].segments.start, 0);
        assert_eq!(
            analysis.paragraphs[0].segments.end,
            analysis.paragraphs[1].segments.start
        );
        assert_eq!(analysis.paragraphs[1].segments.end, analysis.segments.len());
        assert!(analysis.paragraphs.iter().all(|p| !p.segments.is_empty()));
    }

    #[test]
    fn paragraph_boundaries_split_segment_stream() {
        let text = "abc\ndef";
        let analysis = analyze(text, None);

        assert_eq!(analysis.paragraphs.len(), 2);
        assert!(analysis.segments.len() >= 2);

        let p0 = analysis.paragraphs[0].segments.clone();
        let p1 = analysis.paragraphs[1].segments.clone();
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
        assert_eq!(ltr.paragraphs[0].level, 0);
        assert_eq!(rtl.paragraphs[0].level, 1);
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
                        &text[analysis.clusters.text_range(t.clusters.clone()).unwrap()]
                    );
                    for cluster in analysis.clusters.iter_range(t.clusters.clone()) {
                        dump_cluster(text, &cluster);
                    }
                }
            }
        }
    }
}

#[allow(unused)]
fn dump_cluster(text: &str, cluster: &Cluster) {
    let cluster_text = &text[cluster.text_range.clone()];
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
