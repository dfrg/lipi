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

pub use analysis::{BidiAnalysis, BidiSegment, ClusterAnalysis, Paragraph, TextAnalysis};
pub use analyzer::TextAnalyzer;
pub use bidi::BidiLevel;
pub use cluster::{Cluster, ClusterAttributes, ClusterContent, ClusterRange, WordKind};
pub use element::{SourceElement, SourceElementKind};
pub use parlance::{BidiDirection, BidiOverride, WordBreak};
pub use properties::LineBreak;

#[derive(Clone, PartialEq, Eq, Debug)]
struct PendingCluster {
    attrs: ClusterAttributes,
    range: Range<usize>,
    base_char: char,
    script: Script,
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
        let mut props = props.unwrap_or_default();
        let mut a = TextAnalysis::default();
        TextAnalyzer::default()
            .analyze(
                text,
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
    fn dump_clusters() {
        let text = "a ❤️ a 🏉 rugby  football\tand an 🏈\u{FE0E} american\u{00a0}wut 123 football 🧙🏼‍♀️ ☺ ❤❤️ বিন্ধ্য 🇫🇷 \r\nbb";
        let an = analyze(text, None);
        dump_analysis(text, &an);
        // let clusters = an.cluster_ranges().collect::<Vec<_>>();
        // for cluster in &clusters {
        //     dump_cluster(text, cluster);
        // }
    }

    #[test]
    fn large_clusters() {
        let text = &Some('a')
            .into_iter()
            .chain(core::iter::repeat('\u{0301}').take(40))
            .collect::<String>();
        let an = analyze(text, None);
        let clusters = an.cluster.iter().collect::<Vec<_>>();
        // println!("{:?}", an.clusters);
        println!("{clusters:?}");
        for cluster in &clusters {
            dump_cluster(text, cluster);
        }
    }

    fn analyze_ex(text: &str, props: &[(TextAnalysisProperties, usize)]) -> TextAnalysis {
        let mut a = TextAnalysis::default();
        let mut prop_set = PropSet(props);
        TextAnalyzer::default()
            .analyze(
                text,
                &mut prop_set,
                props
                    .iter()
                    .enumerate()
                    .map(|(i, (_props, len))| SourceElement {
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

    fn analyze_ex2(
        text: &str,
        props: &[(TextAnalysisProperties, SourceElementKind)],
    ) -> TextAnalysis {
        let mut a = TextAnalysis::default();
        let mut prop_set = PropSet(props);
        TextAnalyzer::default()
            .analyze(
                text,
                &mut prop_set,
                props
                    .iter()
                    .enumerate()
                    .map(|(i, (_props, el))| SourceElement {
                        handle: ElementHandle {
                            id: i as u64,
                            context_id: 0,
                        },
                        kind: *el,
                    }),
                &mut a,
            )
            .unwrap();
        a
    }

    struct PropSet<'a, T>(&'a [(TextAnalysisProperties, T)]);

    impl<'a, T> TextAnalysisPropertiesProvider for PropSet<'a, T> {
        fn text_analysis_properties(&mut self, handle: &ElementHandle) -> TextAnalysisProperties {
            self.0.get(handle.id as usize).map(|p| p.0).unwrap()
        }
    }

    #[test]
    fn line_breaks() {
        let text = "Hello world";
        const fn mk_props(wb: WordBreak, len: usize) -> (TextAnalysisProperties, usize) {
            (
                TextAnalysisProperties {
                    line_break: LineBreak::Normal,
                    word_break: wb,
                    language: None,
                },
                len,
            )
        }
        let an = analyze_ex(
            text,
            &[
                mk_props(WordBreak::Normal, 3),
                mk_props(WordBreak::BreakAll, 5),
                mk_props(WordBreak::Normal, 100),
            ],
        );
        let clusters = an.cluster.iter().collect::<Vec<_>>();
        for cluster in &clusters {
            dump_cluster(text, cluster);
        }
    }

    #[test]
    fn object_replacement() {
        let text = "helloobjworld";
        let ar = analyze_ex2(
            text,
            &[
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(5),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Object(parlance::BidiDirection::Auto, 1),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Object(parlance::BidiDirection::Auto, 2),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(100),
                ),
            ],
        );
        dump_analysis(text, &ar);
    }

    #[test]
    fn objects() {
        let text = "helloworld";
        let ar = analyze_ex2(
            text,
            &[
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(5),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Object(parlance::BidiDirection::Auto, 0),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(100),
                ),
            ],
        );
        dump_analysis(text, &ar);
    }

    #[test]
    fn object_replacement2() {
        let text = "abc";
        let ar = analyze_ex2(
            text,
            &[
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(1),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Object(parlance::BidiDirection::Auto, 1),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(1),
                ),
            ],
        );
        // println!("{:?}", &ar.clusters);
        dump_analysis(text, &ar);
    }

    #[test]
    fn objects2() {
        let text = "acb\r\n";
        let ar = analyze_ex2(
            text,
            &[
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(1),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Object(parlance::BidiDirection::Auto, 1),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(1),
                ),
            ],
        );
        // println!("{:?}", &ar.clusters);
        dump_analysis(text, &ar);
    }

    #[test]
    fn break_shaping() {
        let text = "a\u{0301}b";
        let ar = analyze_ex2(
            text,
            &[
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(1),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::BreakSegmentation,
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(2),
                ),
            ],
        );
        // println!("{:?}", &ar.clusters);
        dump_analysis(text, &ar);
    }

    #[test]
    fn bidi_stuff() {
        let text = "a\u{0301}bcde";
        let ar = analyze_ex2(
            text,
            &[
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(1),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::PushBidiIsolate(parlance::BidiDirection::Rtl),
                    // SourceElementKind::PushBidiOverride(parlance::BidiOverride::Rtl),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(2),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Object(BidiDirection::Rtl, 0),
                ),
                (
                    TextAnalysisProperties::default(),
                    // SourceElementKind::PopBidiOverride,
                    SourceElementKind::PopBidiIsolate,
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(2),
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::BreakSegmentation,
                ),
            ],
        );
        // println!("{:?}", &ar.clusters);
        dump_analysis(text, &ar);
    }

    fn dump_analysis(text: &str, analysis: &TextAnalysis) {
        for cluster in analysis.cluster.iter() {
            dump_cluster(text, &cluster);
        }
        println!("");
        for ss in &analysis.cluster.script_segments {
            println!("[{}] {}", ss.script, &text[ss.range.text.clone()]);
            println!("{ss:?}");
            for cluster in analysis.cluster.iter_range(&ss.range) {
                dump_cluster(text, &cluster);
            }
        }
    }
}

#[allow(unused)]
fn dump_cluster(text: &str, cluster: &Cluster) {
    let cluster_text = &text[cluster.text_range.clone()];
    let replacement = if cluster.is_replaced {
        " <replaced>"
    } else {
        ""
    };
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
    println!("[{content} {line} {word} {rtl}]:     {cluster_text:?} ({cluster_text}) ({count} chars) {replacement}");
}
