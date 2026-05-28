//! Text analysis context.

use super::{
    bidi::{self, BidiBracket, BidiClass},
    is_real_script,
    unicode::{
        CharProperties, UnicodeEngine, UnicodeSegmentationContext, UnicodeSegmentationCursor,
    },
    BidiDirection, BidiOverride, ClusterAttributes, Paragraph, PendingCluster, Segment,
    SourceElement, SourceElementKind, TextAnalysis, TextAnalysisProperties,
    TextAnalysisPropertiesProvider, TextSegment, WordKind,
};
#[cfg(feature = "icu")]
use super::unicode::IcuUnicodeEngine;
use crate::{
    text::BidiControl, Element, ElementKind, Language, ObjectHandle, Script, MAX_TEXT_LEN,
};
use alloc::vec::Vec;
use core::ops::Range;

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
    paragraph_bidi_ranges: Vec<ParagraphBidiRange>,
    bidi_items: Vec<BidiItem>,
    text_segments: Vec<TextSegment>,
}

#[derive(Clone)]
struct ParagraphBidiRange {
    bidi: Range<usize>,
    brackets: Range<usize>,
}

#[derive(Copy, Clone, Default)]
struct TransitionEffects {
    reset_line_iter: bool,
    reset_grapheme_word_iters: bool,
    next_text_start: usize,
}

struct ScanCtx {
    element_start: usize,
    element_end: usize,
    properties: TextAnalysisProperties,
    pending_cluster: PendingCluster,
}

struct AnalyzeState {
    scan: ScanCtx,
    needs_bidi: bool,
    num_objects: u32,
    last_text_end: usize,
    paragraph_bidi_start: usize,
    paragraph_bracket_start: usize,
    unresolved_script_segments: usize,
    prev_cluster_is_paragraph_separator: bool,
}

impl TextAnalyzer {
    #[cfg(feature = "icu")]
    pub fn analyze(
        &mut self,
        text: &str,
        base_direction: BidiDirection,
        property_provider: &mut impl TextAnalysisPropertiesProvider,
        elements: impl Iterator<Item = SourceElement>,
        analysis: &mut TextAnalysis,
    ) -> Result<(), TextAnalysisError> {
        self.analyze_with_unicode_engine(
            text,
            base_direction,
            &IcuUnicodeEngine,
            property_provider,
            elements,
            analysis,
        )
    }

    pub fn analyze_with_unicode_engine<E>(
        &mut self,
        text: &str,
        base_direction: BidiDirection,
        unicode_engine: &E,
        property_provider: &mut impl TextAnalysisPropertiesProvider,
        mut elements: impl Iterator<Item = SourceElement>,
        analysis: &mut TextAnalysis,
    ) -> Result<(), TextAnalysisError>
    where
        E: UnicodeEngine,
    {
        self.clear();
        analysis.clear();
        if text.len() > MAX_TEXT_LEN {
            return Err(TextAnalysisError::TextExceedsMaxLen);
        }
        let mut chars = text
            .char_indices()
            .chain(Some((text.len(), ' ')))
            .enumerate();
        // Start before the first element so all source elements, including
        // leading zero-length ones (e.g. objects/controls), are processed by
        // the same transition logic below.
        let mut state = AnalyzeState {
            scan: ScanCtx {
                element_start: 0,
                element_end: 0,
                properties: TextAnalysisProperties::default(),
                pending_cluster: PendingCluster {
                    attrs: ClusterAttributes::default(),
                    range: 0..0,
                    base_char: ' ',
                    bidi_class: BidiClass::OTHER_NEUTRAL,
                    bidi_bracket: None,
                    script: Script::COMMON,
                    lang: None,
                },
            },
            needs_bidi: false,
            num_objects: 0,
            last_text_end: 0,
            paragraph_bidi_start: 0,
            paragraph_bracket_start: 0,
            unresolved_script_segments: 0,
            prev_cluster_is_paragraph_separator: false,
        };
        state.scan.pending_cluster.lang = state.scan.properties.language;
        let context = unicode_engine.segmentation_context();
        let mut boundaries = context.cursor(text, state.scan.properties);
        let mut grapheme_start = state.scan.element_start;
        let mut word_start = state.scan.element_start;
        let mut line_start = state.scan.element_start;
        let mut graphemes = BoundaryTracker::new(state.scan.element_start);
        let mut words = WordBoundaryTracker::new(state.scan.element_start);
        let mut lines = BoundaryTracker::new(state.scan.element_start);
        while let Some((_char_idx, (byte_idx, ch))) = chars.next() {
            let char_props = unicode_engine.char_properties(ch);
            // Do we need to move on to the next element?
            if byte_idx >= state.scan.element_end {
                let mut next_word_kind = |idx: usize| {
                    words.is_boundary(idx, || {
                        boundaries
                            .next_word()
                            .map(|(ix, kind)| (ix + word_start, kind))
                    });
                    words.current_kind()
                };
                let transition = self.process_element_boundary(
                    &mut state,
                    property_provider,
                    &mut elements,
                    analysis,
                    byte_idx,
                    ch,
                    &mut next_word_kind,
                );
                let next_text = if transition.reset_grapheme_word_iters | transition.reset_line_iter
                {
                    text.get(transition.next_text_start..).unwrap_or_default()
                } else {
                    ""
                };
                if transition.reset_grapheme_word_iters {
                    boundaries.reset_text_boundaries(&context, next_text);
                    grapheme_start = transition.next_text_start;
                    word_start = transition.next_text_start;
                    graphemes = BoundaryTracker::new(transition.next_text_start);
                    words = WordBoundaryTracker::new(transition.next_text_start);
                }
                if transition.reset_line_iter {
                    boundaries.reset_line_boundaries(&context, next_text, state.scan.properties);
                    line_start = transition.next_text_start;
                    lines = BoundaryTracker::new(transition.next_text_start);
                }
            }
            // Now handle the next grapheme
            if graphemes.is_boundary(byte_idx, || {
                boundaries.next_grapheme().map(|ix| ix + grapheme_start)
            }) {
                let mut cluster = state.scan.pending_cluster.clone();
                // Does it end a word?
                if words.is_boundary(byte_idx, || {
                    boundaries
                        .next_word()
                        .map(|(ix, kind)| (ix + word_start, kind))
                }) {
                    cluster.attrs.set_word_kind(words.current_kind());
                }
                // Is it a line break opportunity?
                if lines.is_boundary(byte_idx, || boundaries.next_line().map(|ix| ix + line_start)) {
                    cluster.attrs.set_line_break();
                }
                cluster.range.end = byte_idx;
                state.scan.pending_cluster.reset(
                    ch,
                    char_props,
                    state.scan.properties.language,
                    byte_idx,
                );
                if byte_idx > 0 && !cluster.range.is_empty() {
                    self.push_cluster(&mut state, analysis, &cluster);
                }
            } else {
                state.scan.pending_cluster.attrs.update_content(ch);
            }
        }
        let bidi_base_level = match base_direction {
            BidiDirection::Ltr => Some(0),
            BidiDirection::Rtl => Some(1),
            _ => None,
        };
        if self.bidi_classes.len() > state.paragraph_bidi_start {
            self.paragraph_bidi_ranges.push(ParagraphBidiRange {
                bidi: state.paragraph_bidi_start..self.bidi_classes.len(),
                brackets: state.paragraph_bracket_start..self.bidi_brackets.len(),
            });
        }
        if state.unresolved_script_segments != 0 {
            self.back_propagate_unresolved_scripts();
        }
        self.handle_bidi(analysis, bidi_base_level);
        Ok(())
    }
}

impl TextAnalyzer {
    fn process_element_boundary<F>(
        &mut self,
        state: &mut AnalyzeState,
        property_provider: &mut impl TextAnalysisPropertiesProvider,
        elements: &mut impl Iterator<Item = SourceElement>,
        analysis: &mut TextAnalysis,
        byte_idx: usize,
        ch: char,
        next_word_kind: &mut F,
    ) -> TransitionEffects
    where
        F: FnMut(usize) -> WordKind,
    {
        // Track whether we need to reset segmentation iterators.
        let mut effects = TransitionEffects::default();
        effects.next_text_start = state.scan.element_start;
        state.scan.pending_cluster.range.end = byte_idx;

        // We need to skip zero length elements and objects.
        loop {
            if let Some(next_element) = elements.next() {
                let next_properties =
                    property_provider.text_analysis_properties(&next_element.handle);
                let text_start = state.scan.element_end as u32;
                state.scan.element_start = state.scan.element_end;
                effects.next_text_start = state.scan.element_start;
                // Synthesized bidi control character.
                let mut pending_bidi = None;
                // Most elements flush the pending cluster so default to true.
                let mut flush_pending = true;
                let mut next_len = 0usize;
                match next_element.kind {
                    SourceElementKind::Text(len) => {
                        analysis.elements.push(Element {
                            handle: next_element.handle,
                            kind: ElementKind::Text(len),
                            text_start,
                        });
                        next_len = len as usize;
                    }
                    SourceElementKind::Object(dir) => {
                        let object_handle = ObjectHandle(state.num_objects);
                        state.num_objects += 1;
                        analysis.elements.push(Element {
                            handle: next_element.handle,
                            kind: ElementKind::Object(object_handle),
                            text_start,
                        });
                        // Objects reset all iterators.
                        effects.reset_line_iter = true;
                        effects.reset_grapheme_word_iters = true;
                        let class = match dir {
                            BidiDirection::Auto => BidiClass::OTHER_NEUTRAL,
                            BidiDirection::Ltr => BidiClass::LEFT_TO_RIGHT,
                            BidiDirection::Rtl => BidiClass::RIGHT_TO_LEFT,
                        };
                        pending_bidi = Some((class, BidiItem::Object(object_handle)));
                        flush_pending = true;
                    }
                    SourceElementKind::StartSpan => {
                        analysis.elements.push(Element {
                            handle: next_element.handle,
                            kind: ElementKind::StartSpan,
                            text_start,
                        });
                        flush_pending = false;
                    }
                    SourceElementKind::EndSpan => {
                        analysis.elements.push(Element {
                            handle: next_element.handle,
                            kind: ElementKind::EndSpan,
                            text_start,
                        });
                        flush_pending = false;
                    }
                    SourceElementKind::Marker => {
                        analysis.elements.push(Element {
                            handle: next_element.handle,
                            kind: ElementKind::Marker,
                            text_start,
                        });
                        flush_pending = false;
                    }
                    SourceElementKind::BidiControl(control) => match control {
                        _ => {
                            state.needs_bidi = true;
                        }
                    },
                    SourceElementKind::SegmentationBreak => {
                        effects.reset_line_iter = true;
                        effects.reset_grapheme_word_iters = true;
                        // The break item won't push a class so the actual value is irrelevant.
                        pending_bidi = Some((BidiClass::OTHER_NEUTRAL, BidiItem::Break));
                    }
                };

                if let SourceElementKind::BidiControl(control) = next_element.kind {
                    match control {
                        BidiControl::PushOverride(dir) => {
                            let class = match dir {
                                BidiOverride::Ltr => BidiClass::LEFT_TO_RIGHT_OVERRIDE,
                                BidiOverride::Rtl => BidiClass::RIGHT_TO_LEFT_OVERRIDE,
                            };
                            pending_bidi = Some((class, BidiItem::Control));
                        }
                        BidiControl::PopOverride => {
                            pending_bidi =
                                Some((BidiClass::POP_DIRECTIONAL_FORMAT, BidiItem::Control));
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
                            pending_bidi =
                                Some((BidiClass::POP_DIRECTIONAL_ISOLATE, BidiItem::Control));
                        }
                    }
                }

                state.scan.element_end = state.scan.element_end.saturating_add(next_len);
                if next_len > 0 {
                    // Reset the line iterator if we forced a grapheme/word reset or if
                    // the properties changed.
                    effects.reset_line_iter = effects.reset_grapheme_word_iters
                        || state.scan.properties != next_properties;
                    state.scan.properties = next_properties;
                    // We found an element that consumes some characters.
                    break;
                }

                if flush_pending && !state.scan.pending_cluster.range.is_empty() {
                    let word_kind = next_word_kind(byte_idx);
                    self.flush_pending_cluster(
                        state,
                        analysis,
                        word_kind,
                        byte_idx,
                        ch,
                        state.scan.properties.language,
                    );
                }
                // Handle synthesized bidi control characters. This must be done
                // _after_ flushing the pending cluster.
                if let Some((class, item)) = pending_bidi {
                    if !matches!(item, BidiItem::Break) {
                        self.bidi_classes.push(class);
                    }
                    self.bidi_items.push(item);
                }
            } else {
                // We don't have any remaining elements; keep using the current
                // one until we run out of text.
                state.scan.element_end = usize::MAX;
                break;
            }
        }

        effects
    }

    fn flush_pending_cluster(
        &mut self,
        state: &mut AnalyzeState,
        analysis: &mut TextAnalysis,
        word_kind: WordKind,
        byte_idx: usize,
        next_char: char,
        next_language: Option<Language>,
    ) {
        state.scan.pending_cluster.attrs.set_word_kind(word_kind);
        state.scan.pending_cluster.range.end = byte_idx;
        let cluster = state.scan.pending_cluster.clone();
        self.push_cluster(state, analysis, &cluster);
        state.scan.pending_cluster.base_char = next_char;
        state.scan.pending_cluster.range.start = byte_idx;
        state.scan.pending_cluster.lang = next_language;
    }

    fn back_propagate_unresolved_scripts(&mut self) {
        let mut next_real_script = None;
        for item in self.bidi_items.iter_mut().rev() {
            if let BidiItem::Text(segment) = item {
                if is_real_script(segment.script) {
                    next_real_script = Some(segment.script);
                } else if let Some(script) = next_real_script {
                    segment.script = script;
                }
            }
        }
    }

    fn clear(&mut self) {
        self.bidi.clear();
        self.bidi_classes.clear();
        self.bidi_brackets.clear();
        self.paragraph_bidi_ranges.clear();
        self.bidi_items.clear();
        self.text_segments.clear();
    }

    fn push_cluster(
        &mut self,
        state: &mut AnalyzeState,
        analysis: &mut TextAnalysis,
        cluster: &PendingCluster,
    ) -> bool {
        if cluster.range.is_empty() {
            return false;
        }
        let paragraph_bidi_start = state.paragraph_bidi_start;
        let cluster_start = analysis.clusters.len();
        analysis.clusters.push(cluster);
        self.push_bidi_char(
            state,
            cluster.base_char,
            cluster.bidi_class,
            cluster.bidi_bracket,
        );
        let cluster_end = analysis.clusters.len();
        let cluster_is_paragraph_separator = cluster.attrs.is_paragraph_separator();
        if cluster_is_paragraph_separator {
            self.paragraph_bidi_ranges.push(ParagraphBidiRange {
                bidi: paragraph_bidi_start..self.bidi_classes.len(),
                brackets: state.paragraph_bracket_start..self.bidi_brackets.len(),
            });
            state.paragraph_bidi_start = self.bidi_classes.len();
            state.paragraph_bracket_start = self.bidi_brackets.len();
        }
        let last_text_end = state.last_text_end;
        state.last_text_end = cluster.range.end;
        if let Some(BidiItem::Text(last_segment)) = self.bidi_items.last_mut() {
            if cluster.range.start == last_text_end
                && cluster.lang == last_segment.language
                && !cluster_is_paragraph_separator
                && !state.prev_cluster_is_paragraph_separator
            {
                if let Some(merged) = merge_scripts(last_segment.script, cluster.script) {
                    let was_unresolved = !is_real_script(last_segment.script);
                    last_segment.script = merged;
                    if was_unresolved && is_real_script(merged) {
                        state.unresolved_script_segments =
                            state.unresolved_script_segments.saturating_sub(1);
                    }
                    last_segment.clusters.end = cluster_end;
                    state.prev_cluster_is_paragraph_separator = cluster_is_paragraph_separator;
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
        if !is_real_script(cluster.script) {
            state.unresolved_script_segments += 1;
        }
        state.prev_cluster_is_paragraph_separator = cluster_is_paragraph_separator;
        true
    }

    fn push_bidi_char(
        &mut self,
        state: &mut AnalyzeState,
        ch: char,
        class: BidiClass,
        bracket: Option<BidiBracket>,
    ) {
        let start = self.bidi_classes.len();
        let para_local_start = start - state.paragraph_bidi_start;
        state.needs_bidi = state.needs_bidi || bidi::needs_bidi_resolution(class);
        if let Some(bracket) = bracket {
            self.bidi_brackets.push((para_local_start, ch, bracket));
        }
        self.bidi_classes.push(class);
    }

    fn handle_bidi(&mut self, analysis: &mut TextAnalysis, base_level: Option<u8>) {
        analysis.segments.clear();
        analysis.paragraphs.clear();

        if !self.bidi_classes.is_empty() {
            let bidi_items = &self.bidi_items;
            let (bidi, bidi_brackets) = (&mut self.bidi, &self.bidi_brackets);
            let mut item_ix = 0;
            let mut force_break = false;
            for paragraph_range in &self.paragraph_bidi_ranges {
                let bidi_range = paragraph_range.bidi.clone();
                let bracket_range = paragraph_range.brackets.clone();
                let brackets = &bidi_brackets[bracket_range];
                bidi.resolve(&self.bidi_classes[bidi_range.clone()], brackets, base_level);

                // Paragraphs are logical segment boundaries.
                if !analysis.segments.is_empty() {
                    force_break = true;
                }
                let segment_start = analysis.segments.len();
                Self::apply_bidi_levels(
                    bidi_items,
                    analysis,
                    bidi.levels(),
                    &mut item_ix,
                    &mut force_break,
                );
                let segment_end = analysis.segments.len();
                analysis.paragraphs.push(Paragraph {
                    level: bidi.base_level(),
                    segments: segment_start..segment_end,
                });
            }
        }

        // Propagate RTL flag to affected clusters.
        for segment in analysis.segments.iter() {
            if let Segment::Text(segment) = segment {
                if segment.bidi_level & 1 != 0 {
                    analysis.clusters.set_rtl(&segment.clusters);
                }
            }
        }
    }

    fn apply_bidi_levels(
        bidi_items: &[BidiItem],
        analysis: &mut TextAnalysis,
        levels: &[u8],
        item_ix: &mut usize,
        force_break: &mut bool,
    ) {
        let segments = &mut analysis.segments;
        let mut remaining_levels = levels;

        while !remaining_levels.is_empty() {
            if *item_ix >= bidi_items.len() {
                break;
            }
            let item = &bidi_items[*item_ix];
            match item {
                BidiItem::Control => {
                    // Controls were inserted for formatting but have no
                    // representation otherwise so just eat the level
                    let Some((_, rest)) = remaining_levels.split_first() else {
                        break;
                    };
                    remaining_levels = rest;
                    *item_ix += 1;
                }
                BidiItem::Object(handle) => {
                    // We only care about the level
                    let Some((&level, rest)) = remaining_levels.split_first() else {
                        break;
                    };
                    segments.push(Segment::Object(level, *handle));
                    remaining_levels = rest;
                    *item_ix += 1;
                }
                BidiItem::Text(segment) => {
                    let count = segment.clusters.len();
                    if remaining_levels.len() < count {
                        break;
                    }
                    let (text_levels, rest) = remaining_levels.split_at(count);
                    // Injected control characters break segments unnecessarily
                    // so try to merge with the previous segment
                    let mut merged = false;
                    let mut segment = if let Some(Segment::Text(last_segment)) = segments.last() {
                        if !*force_break
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
                    *force_break = false;
                    // This is the count of clusters, so sync up while skipping
                    // replacement clusters
                    for (i, level) in text_levels.iter().copied().enumerate() {
                        let cluster_idx = segment.clusters.start + i;
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
                    remaining_levels = rest;
                    *item_ix += 1;
                    if !segment.clusters.is_empty() {
                        segments.push(Segment::Text(segment.clone()));
                    }
                }
                BidiItem::Break => {
                    *force_break = true;
                    *item_ix += 1;
                }
            }
        }
    }
}

impl PendingCluster {
    fn reset(
        &mut self,
        ch: char,
        char_props: CharProperties,
        language: Option<Language>,
        start: usize,
    ) {
        self.range.start = start;
        self.attrs = ClusterAttributes::new(ch, char_props);
        self.bidi_class = char_props.bidi_class;
        self.bidi_bracket = char_props.bidi_bracket;
        let script = char_props.script;
        self.script = if is_real_script(script) {
            script
        } else {
            Script::COMMON
        };
        self.base_char = ch;
        self.lang = language;
    }
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

/// Helper for syncing boundary state tracking between successive indices.
struct BoundaryTracker {
    cur_ix: usize,
}

impl BoundaryTracker {
    fn new(start: usize) -> Self {
        Self { cur_ix: start }
    }

    fn is_boundary<F>(&mut self, ix: usize, mut next_boundary: F) -> bool
    where
        F: FnMut() -> Option<usize>,
    {
        if ix == self.cur_ix {
            return true;
        }
        if ix < self.cur_ix {
            return false;
        }
        while let Some(next_ix) = next_boundary() {
            if next_ix >= ix {
                self.cur_ix = next_ix;
                return next_ix == ix;
            }
        }
        false
    }
}

/// Helper for word boundaries that carry the kind of the preceding segment.
struct WordBoundaryTracker {
    cur_ix: usize,
    cur_kind: WordKind,
}

impl WordBoundaryTracker {
    fn new(start: usize) -> Self {
        Self {
            cur_ix: start,
            cur_kind: WordKind::Other,
        }
    }

    fn is_boundary<F>(&mut self, ix: usize, mut next_boundary: F) -> bool
    where
        F: FnMut() -> Option<(usize, WordKind)>,
    {
        if ix == self.cur_ix {
            return true;
        }
        if ix < self.cur_ix {
            return false;
        }
        while let Some((next_ix, kind)) = next_boundary() {
            if next_ix >= ix {
                self.cur_ix = next_ix;
                self.cur_kind = kind;
                return next_ix == ix;
            }
        }
        false
    }

    fn current_kind(&self) -> WordKind {
        self.cur_kind
    }
}
