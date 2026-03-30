//! Text analysis context.

use super::{
    bidi, ClusterFlags, ClusterRange, PendingCluster, TextAnalysis, TextAnalysisProperties,
    TextAnalysisPropertiesProvider, WordKind,
};
use crate::element::{Element, ElementKind, SourceElement, SourceElementKind};
use crate::properties::script_from_icu;
use crate::{Script, MAX_TEXT_LEN};
use alloc::vec::Vec;
use icu_properties::props::{
    BidiClass, BidiMirroringGlyph, BidiPairedBracketType, EnumeratedProperty,
};
use icu_segmenter::options::WordBreakInvariantOptions;
use parlance::{BidiDirection, BidiOverride};

/// Erros that can occur during text analysis.
#[derive(Clone, Debug)]
pub enum TextAnalysisError {
    /// The input text was larger than the maximum length.
    TextExceedsMaxLen,
}

/// Context for text analysis.
#[derive(Default)]
pub struct TextAnalyzer {
    break_shaping_before: bool,
    bidi: bidi::BidiResolver,
    needs_bidi: bool,
    bidi_classes: Vec<BidiClass>,
    bidi_brackets: Vec<(usize, char, BidiMirroringGlyph)>,
    bidi_items: Vec<BidiItem>,
}

impl TextAnalyzer {
    pub fn analyze(
        &mut self,
        text: &str,
        property_provider: &mut impl TextAnalysisPropertiesProvider,
        mut elements: impl Iterator<Item = SourceElement>,
        analysis: &mut TextAnalysis,
    ) -> Result<(), TextAnalysisError> {
        self.clear();
        analysis.clear();
        if text.len() > MAX_TEXT_LEN {
            return Err(TextAnalysisError::TextExceedsMaxLen);
        }
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
            base_char: ' ',
        };
        // Signifies whether we have processed an object that will replace
        // some text.
        let mut pending_replacement = false;
        while let Some((_char_idx, (byte_idx, ch))) = chars.next() {
            let char_props = parley_data::Properties::get(ch);
            if byte_idx == 0 {
                pending_cluster.script = script_from_icu(char_props.script());
            }
            // Do we need to move on to the next element?
            if byte_idx >= element_end {
                // Track whether we need to reset segmentation iterators
                let mut reset_line_iter = false;
                let mut reset_grapheme_word_iters = false;
                pending_cluster.range.end = byte_idx;
                // We need to skip zero length elements and objects
                loop {
                    if let Some((next_element, next_properties, next_len)) =
                        self.next_element(property_provider, &mut elements, analysis, element_end)
                    {
                        element_start = element_end;
                        element_end = element_end.saturating_add(next_len);
                        // Synthesized bidi control character
                        let mut pending_bidi = None;
                        let flush_replace = pending_replacement;
                        // Most elements flush the pending cluster so default to true
                        let mut flush_pending = true;
                        match next_element.kind {
                            SourceElementKind::Object(dir, _) => {
                                // Objects reset all iterators
                                reset_line_iter = true;
                                reset_grapheme_word_iters = true;
                                let class = match dir {
                                    BidiDirection::Auto => BidiClass::OtherNeutral,
                                    BidiDirection::Ltr => BidiClass::LeftToRight,
                                    BidiDirection::Rtl => BidiClass::RightToLeft,
                                };
                                pending_bidi = Some((class, BidiItem::Object));
                                flush_pending = !pending_replacement;
                                // If the length is non-zero then the object
                                // replaces the text
                                if next_len > 0 {
                                    pending_replacement = true;
                                }
                            }
                            SourceElementKind::PushBidiOverride(dir) => {
                                let class = match dir {
                                    BidiOverride::Ltr => BidiClass::LeftToRightOverride,
                                    BidiOverride::Rtl => BidiClass::RightToLeftOverride,
                                };
                                pending_bidi = Some((class, BidiItem::Control));
                            }
                            SourceElementKind::PopBidiOverride => {
                                pending_bidi =
                                    Some((BidiClass::PopDirectionalFormat, BidiItem::Control));
                            }
                            SourceElementKind::PushBidiIsolate(dir) => {
                                let class = match dir {
                                    BidiDirection::Auto => BidiClass::FirstStrongIsolate,
                                    BidiDirection::Ltr => BidiClass::LeftToRightIsolate,
                                    BidiDirection::Rtl => BidiClass::RightToLeftIsolate,
                                };
                                pending_bidi = Some((class, BidiItem::Control));
                            }
                            SourceElementKind::PopBidiIsolate => {
                                pending_bidi =
                                    Some((BidiClass::PopDirectionalIsolate, BidiItem::Control));
                            }
                            SourceElementKind::BreakSegmentation => {
                                reset_line_iter = true;
                                reset_grapheme_word_iters = true;
                            }
                            _ => {
                                if next_len > 0 {
                                    // Reset the line iterator if we forced a
                                    // grapheme/word reset or if the properties
                                    // changed
                                    reset_line_iter =
                                        reset_grapheme_word_iters || properties != next_properties;
                                    properties = next_properties;
                                    // We found an element that consumes
                                    // some characters
                                    break;
                                }
                                flush_pending = false;
                            }
                        };
                        if flush_pending && !pending_cluster.range.is_empty() {
                            println!("flushing pending with range {:?}", pending_cluster.range);
                            // Advance the word iterator and update the
                            // kind before flushing the cluster. An inline
                            // object always breaks words
                            words.is_boundary(byte_idx);
                            pending_cluster
                                .info
                                .set_word_kind(WordKind::from_icu(words.iter.word_type()));
                            pending_cluster.range.end = byte_idx;
                            if !flush_replace {
                                self.push_bidi_char(
                                    pending_cluster.base_char,
                                    parley_data::Properties::get(pending_cluster.base_char),
                                );
                            }
                            analysis.push_cluster(&pending_cluster, flush_replace);
                            pending_cluster.base_char = ch;
                            pending_cluster.range.start = byte_idx;
                        }
                        // Handle synthesized bidi control characters. This
                        // must be done _after_ flushing the pending cluster
                        if let Some((class, item)) = pending_bidi {
                            self.bidi_classes.push(class);
                            self.bidi_items.push(item);
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
                // Does it end a word?
                if words.is_boundary(byte_idx) {
                    cluster
                        .info
                        .set_word_kind(WordKind::from_icu(words.iter.word_type()));
                }
                // Is it a line break opportunity?
                if lines.is_boundary(byte_idx) {
                    cluster.info.set_line_break();
                }
                cluster.range.end = byte_idx;
                pending_cluster.range.start = byte_idx;
                pending_cluster.info = ClusterFlags::new(ch, char_props);
                pending_cluster.script = script_from_icu(char_props.script());
                pending_cluster.base_char = ch;
                if byte_idx > 0 && !cluster.range.is_empty() {
                    if !pending_replacement {
                        self.push_bidi_char(
                            cluster.base_char,
                            parley_data::Properties::get(cluster.base_char),
                        );
                    }
                    analysis.push_cluster(&cluster, pending_replacement);
                    pending_replacement = false;
                }
            } else {
                pending_cluster.info.update_content(ch);
            }
        }
        self.handle_bidi(text, analysis);
        Ok(())
    }
}

impl TextAnalyzer {
    fn clear(&mut self) {
        self.break_shaping_before = false;
        self.needs_bidi = false;
        self.bidi.clear();
        self.bidi_classes.clear();
        self.bidi_brackets.clear();
        self.bidi_items.clear();
    }

    fn next_element(
        &mut self,
        property_provider: &mut impl TextAnalysisPropertiesProvider,
        elements: &mut impl Iterator<Item = SourceElement>,
        analysis: &mut TextAnalysis,
        text_start: usize,
    ) -> Option<(SourceElement, TextAnalysisProperties, usize)> {
        let text_start = text_start as u32;
        let element = elements.next()?;
        let properties = property_provider.text_analysis_properties(&element.handle);
        let break_shaping_before = self.break_shaping_before;
        self.break_shaping_before = false;
        let len = match element.kind {
            SourceElementKind::Text(len) => {
                analysis.push_element(Element {
                    handle: element.handle,
                    kind: ElementKind::Text(len),
                    text_start,
                    break_shaping_before,
                });
                len
            }
            SourceElementKind::Object(_dir, len) => {
                let object_handle = analysis.next_object();
                analysis.push_element(Element {
                    handle: element.handle,
                    kind: ElementKind::Object(object_handle, len),
                    text_start,
                    break_shaping_before,
                });
                len
            }
            SourceElementKind::StartSpan(id) => {
                analysis.push_element(Element {
                    handle: element.handle,
                    kind: ElementKind::StartSpan(id),
                    text_start,
                    break_shaping_before,
                });
                0
            }
            SourceElementKind::EndSpan(id) => {
                analysis.push_element(Element {
                    handle: element.handle,
                    kind: ElementKind::EndSpan(id),
                    text_start,
                    break_shaping_before,
                });
                0
            }
            SourceElementKind::PushBidiOverride(..)
            | SourceElementKind::PushBidiIsolate(..)
            | SourceElementKind::PopBidiOverride
            | SourceElementKind::PopBidiIsolate => {
                self.needs_bidi = true;
                0
            }
            SourceElementKind::BreakSegmentation => {
                self.break_shaping_before = true;
                0
            }
            SourceElementKind::Marker(id) => {
                analysis.push_element(Element {
                    handle: element.handle,
                    kind: ElementKind::Marker(id),
                    text_start,
                    break_shaping_before,
                });
                0
            }
        };
        Some((element, properties, len as usize))
    }

    fn push_bidi_char(&mut self, ch: char, props: parley_data::Properties) {
        println!("pushing bidi char {ch:?}");
        let class = props.bidi_class();
        let start = self.bidi_classes.len();
        self.needs_bidi = self.needs_bidi || bidi::needs_bidi_resolution(class);
        let bracket = icu_properties::props::BidiMirroringGlyph::for_char(ch);
        if bracket.paired_bracket_type != BidiPairedBracketType::None {
            self.bidi_brackets.push((start, ch, bracket));
        }
        self.bidi_classes.push(class);
        if let Some(BidiItem::Text(count)) = self.bidi_items.last_mut() {
            *count += 1;
        } else {
            self.bidi_items.push(BidiItem::Text(1));
        }
    }

    fn handle_bidi(&mut self, text: &str, analysis: &mut TextAnalysis) {
        if true {
            //self.needs_bidi {
            self.bidi
                .resolve(&self.bidi_classes, &self.bidi_brackets, None);
            println!("bidi_classes = {:?}", self.bidi_classes);
            println!("bidi_levels = {:?}", self.bidi.levels());
            self.apply_bidi(text, analysis, self.bidi.levels().iter().copied());
        } else {
            self.apply_bidi(
                text,
                analysis,
                core::iter::repeat(0).take(self.bidi_classes.len()),
            );
        }
    }

    fn apply_bidi(
        &self,
        text: &str,
        analysis: &mut TextAnalysis,
        mut levels: impl Iterator<Item = u8>,
    ) -> Option<()> {
        let mut segments: Vec<BidiSegment> = Vec::new();
        // Filter replacement clusters
        let mut clusters = analysis
            .clusters()
            .enumerate()
            .filter(|(_, cluster)| !cluster.is_replaced)
            .peekable();
        let mut range = ClusterRange::default();
        // Loop over the bidi items
        for item in &self.bidi_items {
            match item {
                BidiItem::Control => {
                    // Controls were inserted for formatting but have no
                    // representation otherwise so just eat the level
                    let _ = levels.next();
                }
                BidiItem::Object => {
                    // We only care about the level
                    segments.push(BidiSegment::Object(levels.next()?));
                }
                BidiItem::Text(count) => {
                    // This is the count of clusters, so sync up while skipping
                    // replacement clusters
                    let mut cur_level = 0;
                    for i in 0..*count {
                        let level = levels.next().unwrap();
                        // Find the range for the next non-replacement cluster
                        let (cluster_idx, cluster) = clusters.next().unwrap();
                        if i == 0 {
                            range.text.start = cluster.text_range.start;
                            //range.clusters.start = cluster_idx;
                        } else if level != cur_level {
                            segments.push(BidiSegment::Text(cur_level, range.clone()));
                            range.text = cluster.text_range.clone();
                            range.clusters.start = cluster_idx;
                            range.clusters.end = cluster_idx + 1;
                        }
                        cur_level = level;
                        range.text.end = cluster.text_range.end;
                        range.clusters.end = cluster_idx + 1;
                    }
                    if !range.text.is_empty() {
                        range.clusters.end = clusters
                            .peek()
                            .map(|(idx, _)| *idx)
                            .unwrap_or(analysis.num_clusters());
                        segments.push(BidiSegment::Text(cur_level, range.clone()));
                    }
                    range.text.start = range.text.end;
                    range.clusters.start = range.clusters.end;
                }
            }
        }
        core::mem::drop(clusters);
        for segment in &segments {
            if let BidiSegment::Text(level, range) = segment {
                if *level & 1 != 0 {
                    analysis.set_rtl(&range.clusters);
                }
            }
        }
        println!("bidi_segments: {segments:?}");
        Some(())
    }
}

#[derive(Clone, Debug)]
pub enum BidiSegment {
    /// The level and cluster range.
    Text(u8, ClusterRange),
    /// Just the level.
    Object(u8),
}

#[derive(Clone, Debug)]
enum BidiItem {
    Control,
    Object,
    Text(usize),
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
