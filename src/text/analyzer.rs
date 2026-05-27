//! Text analysis context.

use super::{
    bidi::{self, BidiBracket, BidiClass},
    is_real_script,
    properties::script_from_icu,
    BidiDirection, BidiOverride, ClusterAttributes, PendingCluster, Segment, SourceElement,
    SourceElementKind, TextAnalysis, TextAnalysisProperties, TextAnalysisPropertiesProvider,
    TextSegment, WordKind,
};
use crate::{text::BidiControl, Element, ElementKind, ObjectHandle, Script, MAX_TEXT_LEN};
use alloc::vec::Vec;
use {
    icu_properties::props::{BidiMirroringGlyph, BidiPairedBracketType, EnumeratedProperty},
    icu_segmenter::options::WordBreakInvariantOptions,
};

/// Erros that can occur during text analysis.
#[derive(Clone, Debug)]
pub enum TextAnalysisError {
    /// The input text was larger than the maximum length.
    TextExceedsMaxLen,
}

/// Context and scratch memory for text analysis.
#[derive(Default)]
pub struct TextAnalyzer {
    bidi: bidi::BidiResolver,
    bidi_classes: Vec<BidiClass>,
    bidi_brackets: Vec<(usize, char, BidiBracket)>,
    bidi_items: Vec<BidiItem>,
    text_segments: Vec<TextSegment>,
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
        let state = &mut TransientState::default();
        fn real_script(s: icu_properties::props::Script) -> bool {
            use icu_properties::props::Script;
            !matches!(s, Script::Common | Script::Unknown | Script::Inherited)
        }
        let initial_script = text
            .chars()
            .map(|c| parley_data::Properties::get(c).script())
            .find(|s| real_script(*s))
            .map(|s| script_from_icu(s))
            .unwrap_or(Script::from_bytes(*b"Latn"));
        let mut chars = text
            .char_indices()
            .chain(Some((text.len(), ' ')))
            .enumerate();
        let mut element_start = 0;
        // Read the first element
        let (_, mut properties, mut element_end) = self
            .next_element(
                state,
                property_provider,
                &mut elements,
                analysis,
                element_start,
            )
            .unwrap_or_else(|| (Default::default(), Default::default(), usize::MAX));
        // Build our initial iterator set
        let grapheme_breaker = icu_segmenter::GraphemeClusterSegmenter::new();
        let mut graphemes = BoundaryTracker::new(grapheme_breaker.segment_str(text), 0);
        let word_breaker =
            icu_segmenter::WordSegmenter::new_auto(WordBreakInvariantOptions::default());
        let mut words = BoundaryTracker::new(word_breaker.segment_str(text), 0);
        let mut line_options = properties.line_break_options();
        let mut line_breaker = icu_segmenter::LineSegmenter::new_auto(line_options.get());
        let mut lines = BoundaryTracker::new(line_breaker.segment_str(text), 0);
        let mut pending_cluster = PendingCluster {
            attrs: ClusterAttributes::default(),
            range: 0..0,
            base_char: ' ',
            script: initial_script,
            lang: properties.language,
        };
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
                    if let Some((next_element, next_properties, next_len)) = self.next_element(
                        state,
                        property_provider,
                        &mut elements,
                        analysis,
                        element_end,
                    ) {
                        element_start = element_end;
                        element_end = element_end.saturating_add(next_len);
                        // Synthesized bidi control character
                        let mut pending_bidi = None;
                        // Most elements flush the pending cluster so default to true
                        let mut flush_pending = true;
                        match next_element.kind {
                            SourceElementKind::Object(dir) => {
                                // Objects reset all iterators
                                reset_line_iter = true;
                                reset_grapheme_word_iters = true;
                                let class = match dir {
                                    BidiDirection::Auto => BidiClass::OTHER_NEUTRAL,
                                    BidiDirection::Ltr => BidiClass::LEFT_TO_RIGHT,
                                    BidiDirection::Rtl => BidiClass::RIGHT_TO_LEFT,
                                };
                                let handle = ObjectHandle(state.num_objects - 1);
                                pending_bidi = Some((class, BidiItem::Object(handle)));
                                flush_pending = true;
                            }
                            SourceElementKind::BidiControl(control) => match control {
                                BidiControl::PushOverride(dir) => {
                                    let class = match dir {
                                        BidiOverride::Ltr => BidiClass::LEFT_TO_RIGHT_OVERRIDE,
                                        BidiOverride::Rtl => BidiClass::RIGHT_TO_LEFT_OVERRIDE,
                                    };
                                    pending_bidi = Some((class, BidiItem::Control));
                                }
                                BidiControl::PopOverride => {
                                    pending_bidi = Some((
                                        BidiClass::POP_DIRECTIONAL_FORMAT,
                                        BidiItem::Control,
                                    ));
                                }
                                BidiControl::PushIsolate(dir) => {
                                    let class = match dir {
                                        BidiDirection::Auto => BidiClass::FIRST_STRONG_ISOLATE,
                                        BidiDirection::Ltr => BidiClass::LEFT_TO_RIGHT_ISOLATE,
                                        BidiDirection::Rtl => BidiClass::RIGHT_TO_LEFT_ISOLATE,
                                    };
                                    pending_bidi = Some((class, BidiItem::Control));
                                }
                                BidiControl::PopIsolate => {
                                    pending_bidi = Some((
                                        BidiClass::POP_DIRECTIONAL_ISOLATE,
                                        BidiItem::Control,
                                    ));
                                }
                            },
                            SourceElementKind::SegmentationBreak => {
                                reset_line_iter = true;
                                reset_grapheme_word_iters = true;
                                // The break item won't push a class so the
                                // actual value is irrelevant
                                pending_bidi = Some((BidiClass::OTHER_NEUTRAL, BidiItem::Break));
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
                            // Advance the word iterator so we capture the
                            // correct type
                            words.is_boundary(byte_idx);
                            pending_cluster
                                .attrs
                                .set_word_kind(WordKind::from_icu(words.iter.word_type()));
                            pending_cluster.range.end = byte_idx;
                            self.push_cluster(state, analysis, &pending_cluster);
                            pending_cluster.base_char = ch;
                            pending_cluster.range.start = byte_idx;
                            pending_cluster.lang = properties.language;
                        }
                        // Handle synthesized bidi control characters. This
                        // must be done _after_ flushing the pending cluster
                        if let Some((class, item)) = pending_bidi {
                            if !matches!(item, BidiItem::Break) {
                                self.bidi_classes.push(class);
                            }
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
                        .attrs
                        .set_word_kind(WordKind::from_icu(words.iter.word_type()));
                }
                // Is it a line break opportunity?
                if lines.is_boundary(byte_idx) {
                    cluster.attrs.set_line_break();
                }
                cluster.range.end = byte_idx;
                pending_cluster.range.start = byte_idx;
                pending_cluster.attrs = ClusterAttributes::new(ch, char_props);
                pending_cluster.script = script_from_icu(char_props.script());
                pending_cluster.base_char = ch;
                pending_cluster.lang = properties.language;
                if byte_idx > 0 && !cluster.range.is_empty() {
                    self.push_cluster(state, analysis, &cluster);
                }
            } else {
                pending_cluster.attrs.update_content(ch);
            }
        }
        self.handle_bidi(analysis);
        Ok(())
    }
}

impl TextAnalyzer {
    fn clear(&mut self) {
        self.bidi.clear();
        self.bidi_classes.clear();
        self.bidi_brackets.clear();
        self.bidi_items.clear();
        self.text_segments.clear();
    }

    fn next_element(
        &mut self,
        state: &mut TransientState,
        property_provider: &mut impl TextAnalysisPropertiesProvider,
        elements: &mut impl Iterator<Item = SourceElement>,
        analysis: &mut TextAnalysis,
        text_start: usize,
    ) -> Option<(SourceElement, TextAnalysisProperties, usize)> {
        let text_start = text_start as u32;
        let element = elements.next()?;
        let properties = property_provider.text_analysis_properties(&element.handle);
        let break_shaping_before = state.break_shaping_before;
        state.break_shaping_before = false;
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
            SourceElementKind::Object(..) => {
                let object_handle = ObjectHandle(state.num_objects);
                state.num_objects += 1;
                analysis.elements.push(Element {
                    handle: element.handle,
                    kind: ElementKind::Object(object_handle),
                    text_start,
                    break_shaping_before,
                });
                0
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
            SourceElementKind::BidiControl(..) => {
                state.needs_bidi = true;
                0
            }
            SourceElementKind::SegmentationBreak => {
                state.break_shaping_before = true;
                0
            }
            SourceElementKind::Marker => {
                analysis.elements.push(Element {
                    handle: element.handle,
                    kind: ElementKind::Marker,
                    text_start,
                    break_shaping_before,
                });
                0
            }
        };
        Some((element, properties, len as usize))
    }

    fn push_cluster(
        &mut self,
        state: &mut TransientState,
        analysis: &mut TextAnalysis,
        cluster: &PendingCluster,
    ) -> bool {
        if cluster.range.is_empty() {
            return false;
        }
        let cluster_start = analysis.clusters.len();
        analysis.clusters.push(cluster);
        self.push_bidi_char(
            state,
            cluster.base_char,
            parley_data::Properties::get(cluster.base_char),
        );
        let cluster_end = analysis.clusters.len();
        let last_text_end = state.last_text_end;
        state.last_text_end = cluster.range.end;
        if let Some(BidiItem::Text(last_segment)) = self.bidi_items.last_mut() {
            if cluster.range.start == last_text_end && cluster.lang == last_segment.language {
                if let Some(merged) = merge_scripts(last_segment.script, cluster.script) {
                    last_segment.script = merged;
                    last_segment.clusters.end = cluster_end;
                    return true;
                }
            }
        }
        self.bidi_items.push(BidiItem::Text(TextSegment {
            script: cluster.script,
            language: cluster.lang,
            bidi_level: 0,
            clusters: cluster_start..cluster_end,
        }));
        true
    }

    fn push_bidi_char(
        &mut self,
        state: &mut TransientState,
        ch: char,
        props: parley_data::Properties,
    ) {
        println!("pushing bidi char {ch:?}");
        let class = BidiClass::from_icu4c_value(props.bidi_class().to_icu4c_value() as u8);
        let start = self.bidi_classes.len();
        state.needs_bidi = state.needs_bidi || bidi::needs_bidi_resolution(class);
        if let Some(bracket) = bidi_bracket_from_icu(ch) {
            self.bidi_brackets.push((start, ch, bracket));
        }
        self.bidi_classes.push(class);
    }

    fn handle_bidi(&mut self, analysis: &mut TextAnalysis) {
        if true {
            //self.needs_bidi {
            self.bidi
                .resolve(&self.bidi_classes, &self.bidi_brackets, None);
            println!("bidi_classes = {:?}", self.bidi_classes);
            println!("bidi_levels = {:?}", self.bidi.levels());
            self.apply_bidi(analysis, self.bidi.levels().iter().copied());
        } else {
            self.apply_bidi(
                analysis,
                core::iter::repeat(0).take(self.bidi_classes.len()),
            );
        }
    }

    fn apply_bidi(
        &self,
        analysis: &mut TextAnalysis,
        mut levels: impl Iterator<Item = u8>,
    ) -> Option<()> {
        let segments = &mut analysis.segments;
        segments.clear();
        let mut cluster_start = 0;
        let mut force_break = false;
        println!("bidi_items = {:?}", self.bidi_items);
        // Loop over the bidi items
        for item in &self.bidi_items {
            match item {
                BidiItem::Control => {
                    // Controls were inserted for formatting but have no
                    // representation otherwise so just eat the level
                    let _ = levels.next();
                }
                BidiItem::Object(handle) => {
                    // We only care about the level
                    segments.push(Segment::Object(levels.next()?, *handle));
                }
                BidiItem::Text(segment) => {
                    let count = segment.clusters.len();
                    // Injected control characters break segments unnecessarily
                    // so try to merge with the previous segment
                    let mut merged = false;
                    let mut segment = if let Some(Segment::Text(last_segment)) = segments.last() {
                        if !force_break
                            && last_segment.script == segment.script
                            && last_segment.language == segment.language
                        {
                            let segment = last_segment.clone();
                            // The logic is simpler if we pop and reinsert
                            // later
                            segments.pop();
                            merged = true;
                            segment
                        } else {
                            segment.clone()
                        }
                    } else {
                        segment.clone()
                    };
                    force_break = false;
                    // This is the count of clusters, so sync up while skipping
                    // replacement clusters
                    for i in 0..count {
                        let level = levels.next().unwrap();
                        let cluster_idx = i + cluster_start;
                        if level != segment.bidi_level {
                            if !merged && i == 0 {
                                // If we didn't merge and this is the first
                                // level we've seen, then just set it
                                segment.bidi_level = level;
                            } else {
                                segments.push(Segment::Text(segment.clone()));
                                segment.bidi_level = level;
                                segment.clusters.start = cluster_idx;
                            }
                        }
                        segment.clusters.end = cluster_idx + 1;
                    }
                    cluster_start += count;
                    if !segment.clusters.is_empty() {
                        segments.push(Segment::Text(segment.clone()));
                    }
                }
                BidiItem::Break => {
                    force_break = true;
                }
            }
        }
        // Propagate RTL flag to affected clusters
        for segment in segments.iter() {
            if let Segment::Text(segment) = segment {
                if segment.bidi_level & 1 != 0 {
                    analysis.clusters.set_rtl(&segment.clusters);
                }
            }
        }
        Some(())
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

/// Transient state needed during analysis.
#[derive(Default)]
struct TransientState {
    break_shaping_before: bool,
    needs_bidi: bool,
    num_objects: u32,
    last_text_end: usize,
}

#[derive(Clone, Debug)]
enum BidiItem {
    Control,
    Object(ObjectHandle),
    Text(TextSegment),
    Break,
}

fn merge_scripts(prev: Script, next: Script) -> Option<Script> {
    if prev == next {
        Some(next)
    } else {
        match (is_real_script(prev), is_real_script(next)) {
            (false, false) => Some(next),
            (true, false) => Some(prev),
            (false, true) => Some(next),
            (true, true) => None,
        }
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
