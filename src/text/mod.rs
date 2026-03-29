//! Text analysis.

pub mod bidi;
mod cluster;

use crate::element::{Element, ElementHandle, ElementKind, SourceElement, SourceElementKind};
use crate::properties::{script_from_icu, LineBreakOptions};
use crate::{Language, LineBreak, Script, WordBreak};
use alloc::vec::Vec;
use core::ops::Range;
use icu_segmenter::options::WordBreakInvariantOptions;

pub use cluster::{ClusterContent, ClusterFlags, WordKind};

#[derive(Clone, PartialEq, Eq, Debug)]
struct PendingCluster {
    info: ClusterFlags,
    range: Range<usize>,
    script: Script,
}

enum BidiSegment {
    Text(Range<usize>),
    Control(char),
    Object(char),
}

/// Context for text analysis.
#[derive(Default)]
pub struct TextAnalyzer {
    break_shaping_before: bool,
    needs_bidi: bool,
    bidi_segments: Vec<BidiSegment>,
}

impl TextAnalyzer {
    pub fn analyze(
        &mut self,
        text: &str,
        property_provider: &mut impl TextAnalysisPropertiesProvider,
        mut elements: impl Iterator<Item = SourceElement>,
        analysis: &mut TextAnalysis,
    ) {
        self.clear();
        analysis.clear();
        let grapheme_breaker = icu_segmenter::GraphemeClusterSegmenter::new();
        let word_breaker =
            icu_segmenter::WordSegmenter::new_auto(WordBreakInvariantOptions::default());
        let mut chars = text
            .char_indices()
            .chain(Some((text.len(), ' ')))
            .enumerate();
        let mut element_start = 0;
        let (_, mut properties, mut element_end) = self
            .next_element(property_provider, &mut elements, analysis, element_start)
            .unwrap_or_else(|| (Default::default(), Default::default(), usize::MAX));
        let mut line_options = properties.line_break_options();
        let mut graphemes = BoundaryTracker::new(grapheme_breaker.segment_str(text), 0);
        let mut words = BoundaryTracker::new(word_breaker.segment_str(text), 0);
        let mut line_breaker = icu_segmenter::LineSegmenter::new_auto(line_options.get());
        let mut lines = BoundaryTracker::new(line_breaker.segment_str(text), 0);
        let mut pending_cluster = PendingCluster {
            info: ClusterFlags::default(),
            range: 0..0,
            script: Script::UNKNOWN,
        };
        // Signifies whether we have processed an object that will replace
        // some text.
        let mut pending_replacement = false;
        while let Some((_char_idx, (byte_idx, ch))) = chars.next() {
            let char_props = parley_data::Properties::get(ch);
            // See if we need to run the BiDi algorithm
            self.needs_bidi =
                self.needs_bidi || bidi::needs_bidi_resolution(char_props.bidi_class());
            if byte_idx == 0 {
                pending_cluster.script = script_from_icu(char_props.script());
            }
            // Do we need to move on to the next element?
            if byte_idx >= element_end {
                // Track whether we need to reset segmentation iterators
                let mut reset_grapheme_word_iters = false;
                let mut reset_line_iter = false;
                pending_cluster.range.end = byte_idx;
                // We need to skip zero length elements and objects
                loop {
                    if let Some((next_element, next_properties, next_len)) =
                        self.next_element(property_provider, &mut elements, analysis, element_end)
                    {
                        element_start = element_end;
                        element_end = element_end.saturating_add(next_len);
                        let flush_replace = pending_replacement;
                        let flush_pending = match next_element.kind {
                            SourceElementKind::Object(..) => {
                                reset_grapheme_word_iters = true;
                                reset_line_iter = true;
                                let flush_pending_cluster = !pending_replacement;
                                // If the length is non-zero then the object
                                // replaces the text
                                if next_len > 0 {
                                    pending_replacement = true;
                                }
                               flush_pending_cluster
                            }
                            SourceElementKind::BreakShaping => {
                                reset_grapheme_word_iters = true;
                                reset_line_iter = true;
                                true
                            }
                            _ => {
                                if next_len > 0 {
                                    // Property changes and objects force a
                                    // reset of segmentation iterators
                                    reset_line_iter =
                                        reset_grapheme_word_iters || properties != next_properties;
                                    properties = next_properties;
                                    // We found an element that consumes
                                    // some characters
                                    break;
                                }
                                false
                            }
                        };
                        if flush_pending && !pending_cluster.range.is_empty() {
                            // Advance the word iterator and update the
                            // kind before flushing the cluster. An inline
                            // object always breaks words
                            words.is_boundary(byte_idx);
                            pending_cluster
                                .info
                                .set_word_kind(WordKind::from_icu(words.iter.word_type()));
                            pending_cluster.range.end = byte_idx;
                            analysis.push_cluster(&pending_cluster, flush_replace);
                            pending_cluster.range.start = byte_idx;
                        }
                    } else {
                        // We don't have any remaining elements; keep using
                        // the current one until we run out of text
                        element_end = usize::MAX;
                        break;
                    }
                }
                let next_text = if reset_grapheme_word_iters | reset_line_iter {
                    text.get(element_start..).unwrap_or_default()
                } else {
                    ""
                };
                if reset_grapheme_word_iters {
                    graphemes = BoundaryTracker::new(
                        grapheme_breaker.segment_str(next_text),
                        element_start,
                    );
                    words =
                        BoundaryTracker::new(word_breaker.segment_str(next_text), element_start);
                }
                if reset_line_iter {
                    line_options = properties.line_break_options();
                    line_breaker = icu_segmenter::LineSegmenter::new_auto(line_options.get());
                    lines =
                        BoundaryTracker::new(line_breaker.segment_str(next_text), element_start);
                }
            }
            // Now handle the next grapheme
            if graphemes.is_boundary(byte_idx) {
                let mut cluster = pending_cluster.clone();
                if words.is_boundary(byte_idx) {
                    cluster
                        .info
                        .set_word_kind(WordKind::from_icu(words.iter.word_type()));
                }
                if lines.is_boundary(byte_idx) {
                    cluster.info.set_line_break();
                }
                cluster.range.end = byte_idx;
                pending_cluster.range.start = byte_idx;
                pending_cluster.info = ClusterFlags::new(ch, char_props);
                pending_cluster.script = script_from_icu(char_props.script());
                if byte_idx > 0 {
                    analysis.push_cluster(&cluster, pending_replacement);
                    pending_replacement = false;
                }
            } else {
                pending_cluster.info.update_content(ch);
            }
        }
    }
}

impl TextAnalyzer {
    fn clear(&mut self) {
        self.break_shaping_before = false;
        self.needs_bidi = false;
        self.bidi_segments.clear();
    }

    fn next_element(
        &mut self,
        property_provider: &mut impl TextAnalysisPropertiesProvider,
        elements: &mut impl Iterator<Item = SourceElement>,
        analysis: &mut TextAnalysis,
        text_start: usize,
    ) -> Option<(SourceElement, TextAnalysisProperties, usize)> {
        let element = elements.next()?;
        let properties = property_provider.text_analysis_properties(&element.handle);
        let break_shaping_before = self.break_shaping_before;
        self.break_shaping_before = false;
        let len = match element.kind {
            SourceElementKind::Text(len) => {
                analysis.elements.push(Element {
                    handle: element.handle,
                    kind: ElementKind::Text(len),
                    text_start,
                    break_shaping_before,
                });
                len
            }
            SourceElementKind::Object(_dir, len) => {
                let object_handle = analysis.num_objects;
                analysis.num_objects += 1;
                analysis.elements.push(Element {
                    handle: element.handle,
                    kind: ElementKind::Object(object_handle, len),
                    text_start,
                    break_shaping_before,
                });
                len
            }
            SourceElementKind::StartSpan => {
                analysis.elements.push(Element {
                    handle: element.handle,
                    kind: ElementKind::StartSpan,
                    text_start,
                    break_shaping_before,
                });
                0
            }
            SourceElementKind::EndSpan => {
                analysis.elements.push(Element {
                    handle: element.handle,
                    kind: ElementKind::EndSpan,
                    text_start,
                    break_shaping_before,
                });
                0
            }
            SourceElementKind::StartBidiOverride(..)
            | SourceElementKind::StartBidiIsolate(..)
            | SourceElementKind::EndBidi => {
                self.needs_bidi = true;
                0
            }
            SourceElementKind::BreakShaping => {
                self.break_shaping_before = true;
                0
            }
            SourceElementKind::Marker(id) => {
                analysis.elements.push(Element {
                    handle: element.handle,
                    kind: ElementKind::Marker(id),
                    text_start,
                    break_shaping_before,
                });
                0
            }
        };
        Some((element, properties, len))
    }
}

/// Results of text analysis.
#[derive(Clone, Default)]
pub struct TextAnalysis {
    clusters: Vec<(ClusterFlags, u8)>,
    elements: Vec<Element>,
    script_segments: Vec<ScriptBidiSegment>,
    num_objects: usize,
}

/// Fragment of text split by script and bidirectional level.
#[derive(Clone, Debug)]
pub struct ScriptBidiSegment {
    pub script: Script,
    pub text_range: Range<usize>,
    pub cluster_range: Range<usize>,
}

impl TextAnalysis {
    /// Denotes that a cluster has been replaced by an object.
    const CLUSTER_REPLACEMENT: u8 = 0x1;
    /// Denotes that a cluster must parse further entries to compute the
    /// full length.
    const CLUSTER_CONTINUES: u8 = 0x2;
    /// Maximum length of a single cluster entry before it is split
    /// and marked with continuations.
    const MAX_CLUSTER_ENTRY_LEN: usize = 64;
}

impl TextAnalysis {
    /// Clears the analysis data.
    pub fn clear(&mut self) {
        self.clusters.clear();
        self.elements.clear();
        self.script_segments.clear();
        self.num_objects = 0;
    }

    pub fn cluster_ranges(&self) -> impl Iterator<Item = (ClusterFlags, Range<usize>, bool)> + '_ {
        let mut tracking_start = 0;
        let mut clusters = self.clusters.iter().copied();
        core::iter::from_fn(move || {
            let (info, len) = clusters.next()?;
            let start = tracking_start;
            let mut end = start + (len >> 2) as usize;
            let is_replacement = len & Self::CLUSTER_REPLACEMENT != 0;
            if len & Self::CLUSTER_CONTINUES != 0 {
                while let Some((_, len)) = clusters.next() {
                    end += (len >> 2) as usize;
                    if len & Self::CLUSTER_CONTINUES == 0 {
                        break;
                    }
                }
            }
            tracking_start = end;
            Some((info, start..end, is_replacement))
        })
    }

    pub fn cluster_ranges_for_segment(
        &self,
        segment: &ScriptBidiSegment,
    ) -> impl Iterator<Item = (ClusterFlags, Range<usize>, bool)> + '_ {
        let mut tracking_start = segment.text_range.start;
        let mut clusters = self
            .clusters
            .get(segment.cluster_range.clone())
            .unwrap_or_default()
            .iter()
            .copied();
        core::iter::from_fn(move || {
            let (info, len) = clusters.next()?;
            let start = tracking_start;
            let mut end = start + (len >> 2) as usize;
            let is_replacement = len & Self::CLUSTER_REPLACEMENT != 0;
            if len & Self::CLUSTER_CONTINUES != 0 {
                while let Some((_, len)) = clusters.next() {
                    end += (len >> 2) as usize;
                    if len & Self::CLUSTER_CONTINUES == 0 {
                        break;
                    }
                }
            }
            tracking_start = end;
            Some((info, start..end, is_replacement))
        })
    }

    fn push_cluster(&mut self, cluster: &PendingCluster, is_replacement: bool) {
        println!(
            "pushing cluster with text {:?}, replacement: {is_replacement:}",
            cluster.range.clone()
        );
        let cluster_start = self.clusters.len();
        let mut len = cluster.range.len();
        while len > Self::MAX_CLUSTER_ENTRY_LEN {
            self.clusters.push((
                cluster.info,
                (Self::MAX_CLUSTER_ENTRY_LEN as u8) << 2
                    | Self::CLUSTER_CONTINUES
                    | is_replacement as u8,
            ));
            len -= Self::MAX_CLUSTER_ENTRY_LEN;
        }
        if len != 0 {
            self.clusters
                .push((cluster.info, (len as u8) << 2 | is_replacement as u8));
        }
        if !is_replacement {
            let mut next = cluster.script;
            if cluster.info.is_emoji() {
                next = match cluster.info.content() {
                    ClusterContent::Emoji => Script::from_bytes(*b"Zsye"),
                    _ => Script::from_bytes(*b"Zsym"),
                };
            }
            if let Some(last_script_segment) = self.script_segments.last_mut() {
                let (do_merge, script) =
                    if cluster.range.start == last_script_segment.text_range.end {
                        let prev = last_script_segment.script;
                        if prev == next {
                            (true, next)
                        } else {
                            let prev_real = is_real_script(prev);
                            let next_real = is_real_script(next);
                            match (prev_real, next_real) {
                                (false, false) => (true, next),
                                (true, false) => (true, prev),
                                (false, true) => (true, next),
                                (true, true) => (false, next),
                            }
                        }
                    } else {
                        (false, next)
                    };
                if do_merge {
                    last_script_segment.script = script;
                    last_script_segment.cluster_range.end = self.clusters.len();
                    last_script_segment.text_range.end = cluster.range.end;
                } else {
                    let cluster_end = self.clusters.len();
                    self.script_segments.push(ScriptBidiSegment {
                        script,
                        text_range: cluster.range.clone(),
                        cluster_range: cluster_start..cluster_end,
                    })
                }
            } else {
                let cluster_end = self.clusters.len();
                self.script_segments.push(ScriptBidiSegment {
                    script: next,
                    text_range: cluster.range.clone(),
                    cluster_range: cluster_start..cluster_end,
                })
            }
        }
    }
}

/// Properties required for text analysis.
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

/// Helper for syncing boundary state tracking between
/// multiple iterators.
struct BoundaryTracker<T> {
    iter: T,
    offset: usize,
    cur_ix: usize,
}

impl<T> BoundaryTracker<T>
where
    T: Iterator<Item = usize>,
{
    fn new(iter: T, offset: usize) -> Self {
        // The first character is always a valid boundary
        Self {
            iter,
            offset,
            cur_ix: offset,
        }
    }

    /// Is the given byte index a boundary state according to the inner
    /// iterator?
    fn is_boundary(&mut self, ix: usize) -> bool {
        if ix == self.cur_ix {
            true
        } else if ix < self.cur_ix {
            false
        } else {
            while let Some(next_ix) = self.iter.next() {
                let next_ix = next_ix + self.offset;
                if next_ix >= ix {
                    self.cur_ix = next_ix;
                    return next_ix == ix;
                }
            }
            false
        }
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

    fn analyze(text: &str, props: Option<TextAnalysisProperties>) -> TextAnalysis {
        let mut props = props.unwrap_or_default();
        let mut a = TextAnalysis::default();
        TextAnalyzer::default().analyze(
            text,
            &mut props,
            [SourceElement {
                handle: ElementHandle::default(),
                kind: SourceElementKind::Text(text.len()),
            }]
            .iter()
            .copied(),
            &mut a,
        );
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
        let clusters = an.cluster_ranges().collect::<Vec<_>>();
        println!("{:?}", an.clusters);
        println!("{clusters:?}");
        for cluster in &clusters {
            dump_cluster(text, cluster);
        }
    }

    fn analyze_ex(text: &str, props: &[(TextAnalysisProperties, usize)]) -> TextAnalysis {
        let mut a = TextAnalysis::default();
        let mut prop_set = PropSet(props);
        TextAnalyzer::default().analyze(
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
                    kind: SourceElementKind::Text(*len),
                }),
            &mut a,
        );
        a
    }

    fn analyze_ex2(
        text: &str,
        props: &[(TextAnalysisProperties, SourceElementKind)],
    ) -> TextAnalysis {
        let mut a = TextAnalysis::default();
        let mut prop_set = PropSet(props);
        TextAnalyzer::default().analyze(
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
        );
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
        let clusters = an.cluster_ranges().collect::<Vec<_>>();
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
        println!("{:?}", &ar.clusters);
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
        println!("{:?}", &ar.clusters);
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
                    SourceElementKind::BreakShaping,
                ),
                (
                    TextAnalysisProperties::default(),
                    SourceElementKind::Text(2),
                ),
            ],
        );
        println!("{:?}", &ar.clusters);
        dump_analysis(text, &ar);
    }    

    fn dump_analysis(text: &str, analysis: &TextAnalysis) {
        // for cluster in analysis.cluster_ranges() {
        //     dump_cluster(text, &cluster);
        // }
        // println!("");
        for ss in &analysis.script_segments {
            println!("[{}] {}", ss.script, &text[ss.text_range.clone()]);
            println!("{ss:?}");
            for cluster in analysis.cluster_ranges_for_segment(&ss) {
                dump_cluster(text, &cluster);
            }
        }
    }

    fn dump_cluster(text: &str, cluster: &(ClusterFlags, Range<usize>, bool)) {
        let cluster_text = &text[cluster.1.clone()];
        let info = cluster.0;
        let replacement = if cluster.2 { " <replacement>" } else { "" };
        let content = match info.content() {
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
        let line = if info.is_line_break_opportunity() {
            'L'
        } else {
            '_'
        };
        let word = match info.word_kind() {
            Some(WordKind::Letter) => 'l',
            Some(WordKind::Number) => 'n',
            Some(WordKind::Other) => 'o',
            None => '_',
        };
        let count = cluster_text.chars().count();
        println!("[{content} {line} {word}]:     {cluster_text:?} ({cluster_text}) ({count} chars) {replacement}");
    }
}
