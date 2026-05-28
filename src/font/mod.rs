use crate::{
    text::{BidiLevel, Segment, SegmentEventSink, TextAnalysis, TextSegment},
    ElementHandle,
};
use alloc::{string::String, vec::Vec};
#[cfg(feature = "fontique")]
use alloc::boxed::Box;
use core::mem;
use parlance::{FontStyle, FontWeight, FontWidth, GenericFamily, Language, Script};

#[cfg(feature = "fontique")]
use fontique::{
    Attributes as FontiqueAttributes, Collection as FontiqueCollection,
    FallbackKey as FontiqueFallbackKey, GenericFamily as FontiqueGenericFamily,
    QueryFamily as FontiqueQueryFamily, QueryStatus as FontiqueQueryStatus,
    SourceCache as FontiqueSourceCache,
};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct FontHandle {
    pub id: u64,
    pub ttc_index: u32,
}

#[derive(Copy, Clone, Debug)]
pub struct FontMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    pub units_per_em: f32,
}

#[derive(Copy, Clone, Debug)]
pub struct Font {
    handle: FontHandle,
    metrics: FontMetrics,
}

impl Font {
    pub const fn new(handle: FontHandle, metrics: FontMetrics) -> Self {
        Self { handle, metrics }
    }

    pub const fn handle(&self) -> FontHandle {
        self.handle
    }

    pub const fn metrics(&self) -> FontMetrics {
        self.metrics
    }
}

#[derive(Default)]
pub struct FontAnalyzer {
    cluster: InputCluster,
    fonts: SelectedFontSet,
}

impl FontAnalyzer {
    pub fn analyze(
        &mut self,
        text: &str,
        text_analysis: &TextAnalysis,
        selector: &mut impl FontSelector,
        analysis: &mut FontAnalysis,
    ) {
        analysis.clear();
        self.cluster.clear();
        self.fonts.clear();

        let mut merged: Option<FontSegment> = None;

        for segment in &text_analysis.segments {
            let Segment::Text(text_segment) = segment else {
                continue;
            };

            let mut push_segment = |seg: FontSegment| {
                if let Some(prev) = &mut merged {
                    if prev.font == seg.font
                        && prev.bidi_level == seg.bidi_level
                        && prev.script == seg.script
                        && prev.language == seg.language
                        && prev.byte_range.end == seg.byte_range.start
                    {
                        prev.byte_range.end = seg.byte_range.end;
                        return;
                    }
                }
                if let Some(prev) = merged.take() {
                    analysis.segments.push(prev);
                }
                merged = Some(seg);
            };

            let mut sink = FontEventSink {
                text,
                text_analysis,
                text_segment,
                selector,
                cluster: &mut self.cluster,
                selected_fonts: &mut self.fonts,
                push_segment: &mut push_segment,
                current_element: ElementHandle::default(),
                cluster_start: 0,
                char_count: 0,
            };
            text_segment.events(text, text_analysis, &mut sink);
            sink.finish_pending_cluster();
        }
        if let Some(prev) = merged.take() {
            analysis.segments.push(prev);
        }
    }
}

#[derive(Default)]
pub struct FontAnalysis {
    pub segments: Vec<FontSegment>,
}

impl FontAnalysis {
    pub fn clear(&mut self) {
        self.segments.clear();
    }
}

pub struct FontSegment {
    pub bidi_level: BidiLevel,
    pub script: Script,
    pub language: Option<Language>,
    pub font: FontHandle,
    pub metrics: FontMetrics,
    pub byte_range: core::ops::Range<usize>,
}

pub trait FontSelector {
    fn select_font(
        &mut self,
        text: &str,
        analysis: &TextAnalysis,
        cluster: &mut InputCluster,
        set: &mut SelectedFontSet,
    );
}

/// A single font family entry for matching.
#[derive(Clone, Debug)]
pub enum FontFamily {
    Named(String),
    Generic(GenericFamily),
}

/// A single OpenType variation setting (`tag` + `value`).
#[derive(Copy, Clone, Debug)]
pub struct FontVariationSetting {
    pub tag: [u8; 4],
    pub value: f32,
}

/// Properties that drive font selection for an element.
#[derive(Clone, Debug)]
pub struct FontSelectionProperties {
    pub families: Vec<FontFamily>,
    pub weight: FontWeight,
    pub width: FontWidth,
    pub style: FontStyle,
    pub variation_settings: Vec<FontVariationSetting>,
}

impl Default for FontSelectionProperties {
    fn default() -> Self {
        let mut families = Vec::new();
        families.push(FontFamily::Generic(GenericFamily::SansSerif));
        Self {
            families,
            weight: FontWeight::default(),
            width: FontWidth::default(),
            style: FontStyle::default(),
            variation_settings: Vec::new(),
        }
    }
}

/// Interface for mapping an element handle to font selection properties.
pub trait FontSelectionPropertiesProvider {
    fn font_selection_properties(&mut self, handle: &ElementHandle) -> FontSelectionProperties;
}

#[derive(Default)]
pub struct DefaultFontSelectionPropertiesProvider;

impl FontSelectionPropertiesProvider for DefaultFontSelectionPropertiesProvider {
    fn font_selection_properties(&mut self, _handle: &ElementHandle) -> FontSelectionProperties {
        FontSelectionProperties::default()
    }
}

#[cfg(feature = "fontique")]
pub struct FontiqueSelector {
    collection: FontiqueCollection,
    source_cache: FontiqueSourceCache,
    properties_provider: Box<dyn FontSelectionPropertiesProvider>,
}

#[cfg(feature = "fontique")]
impl FontiqueSelector {
    pub fn new(properties_provider: impl FontSelectionPropertiesProvider + 'static) -> Self {
        let mut collection = FontiqueCollection::default();
        collection.load_system_fonts();
        Self {
            collection,
            source_cache: FontiqueSourceCache::default(),
            properties_provider: Box::new(properties_provider),
        }
    }

    pub fn set_properties_provider(
        &mut self,
        properties_provider: impl FontSelectionPropertiesProvider + 'static,
    ) {
        self.properties_provider = Box::new(properties_provider);
    }

    fn query_best_run(
        &mut self,
        chars: &[InputChar],
        script: Script,
        language: Option<Language>,
        props: &FontSelectionProperties,
    ) -> Option<(Font, usize)> {
        let mut query_families = Vec::new();
        if props.families.is_empty() {
            query_families.push(FontiqueQueryFamily::from(FontiqueGenericFamily::SansSerif));
        } else {
            for family in &props.families {
                match family {
                    FontFamily::Named(name) => {
                        query_families.push(FontiqueQueryFamily::Named(name.as_str()));
                    }
                    FontFamily::Generic(generic) => {
                        query_families.push(FontiqueQueryFamily::Generic(*generic));
                    }
                }
            }
        }

        let mut query = self.collection.query(&mut self.source_cache);
        query.set_families(query_families);
        query.set_attributes(FontiqueAttributes::new(props.width, props.style, props.weight));
        query.set_fallbacks(FontiqueFallbackKey::new(script, language.as_ref()));

        let mut best: Option<(Font, usize)> = None;
        let mut fallback: Option<Font> = None;
        query.matches_with(|query_font| {
            let font = Font::new(
                FontHandle {
                    id: query_font.family.0.to_u64(),
                    ttc_index: query_font.index,
                },
                FontMetrics {
                    ascent: 0.0,
                    descent: 0.0,
                    line_gap: 0.0,
                    units_per_em: 1000.0,
                },
            );
            if fallback.is_none() {
                fallback = Some(font);
            }
            if let Some(charmap) = query_font.charmap() {
                let covered = chars
                    .iter()
                    .take_while(|input| charmap.map(input.char as u32).is_some())
                    .count();
                if covered > 0 {
                    best = Some((font, covered));
                    return FontiqueQueryStatus::Stop;
                }
            }
            FontiqueQueryStatus::Continue
        });

        best.or_else(|| fallback.map(|font| (font, 1)))
    }
}

#[cfg(feature = "fontique")]
impl Default for FontiqueSelector {
    fn default() -> Self {
        Self::new(DefaultFontSelectionPropertiesProvider)
    }
}

#[cfg(feature = "fontique")]
impl FontSelector for FontiqueSelector {
    fn select_font(
        &mut self,
        _text: &str,
        _analysis: &TextAnalysis,
        cluster: &mut InputCluster,
        set: &mut SelectedFontSet,
    ) {
        let chars = cluster.chars();
        let script = cluster.script().unwrap_or(Script::COMMON);
        let language = cluster.language();
        let mut cursor = 0;
        while cursor < chars.len() {
            let handle = chars[cursor].element_handle;
            let props = self.properties_provider.font_selection_properties(&handle);

            let mut element_end = cursor + 1;
            while element_end < chars.len() && chars[element_end].element_handle == handle {
                element_end += 1;
            }

            let mut remaining = &chars[cursor..element_end];
            while !remaining.is_empty() {
                if let Some((font, num_chars)) = self.query_best_run(remaining, script, language, &props)
                {
                    let consumed = num_chars.max(1).min(remaining.len());
                    set.push(font, consumed);
                    remaining = &remaining[consumed..];
                } else {
                    set.push(
                        Font::new(
                            FontHandle {
                                id: 0,
                                ttc_index: 0,
                            },
                            FontMetrics {
                                ascent: 0.0,
                                descent: 0.0,
                                line_gap: 0.0,
                                units_per_em: 1000.0,
                            },
                        ),
                        1,
                    );
                    remaining = &remaining[1..];
                }
            }

            cursor = element_end;
        }
    }
}

#[derive(Default)]
pub struct InputCluster {
    chars: Vec<InputChar>,
    script: Option<Script>,
    language: Option<Language>,
}

impl InputCluster {
    pub fn chars(&self) -> &[InputChar] {
        &self.chars
    }

    pub fn clear(&mut self) {
        self.chars.clear();
    }

    pub fn script(&self) -> Option<Script> {
        self.script
    }

    pub fn language(&self) -> Option<Language> {
        self.language
    }

    fn set_context(&mut self, script: Script, language: Option<Language>) {
        self.script = Some(script);
        self.language = language;
    }

    fn push(&mut self, input: InputChar) {
        self.chars.push(input);
    }
}

#[derive(Copy, Clone, Debug)]
pub struct InputChar {
    pub char: char,
    pub element_handle: ElementHandle,
}

#[derive(Default)]
pub struct SelectedFontSet {
    fonts: Vec<(Font, usize)>,
}

impl SelectedFontSet {
    pub fn push(&mut self, font: Font, num_chars: usize) {
        self.fonts.push((font, num_chars));
    }

    pub fn clear(&mut self) {
        self.fonts.clear();
    }

    fn take(&mut self) -> Vec<(Font, usize)> {
        mem::take(&mut self.fonts)
    }
}

struct FontEventSink<'a, S: FontSelector, F: FnMut(FontSegment)> {
    text: &'a str,
    text_analysis: &'a TextAnalysis,
    text_segment: &'a TextSegment,
    selector: &'a mut S,
    cluster: &'a mut InputCluster,
    selected_fonts: &'a mut SelectedFontSet,
    push_segment: F,
    current_element: ElementHandle,
    cluster_start: usize,
    char_count: usize,
}

impl<S: FontSelector, F: FnMut(FontSegment)> FontEventSink<'_, S, F> {
    fn finish_pending_cluster(&mut self) {
        if self.cluster.chars().is_empty() {
            return;
        }

        // Clone chars to avoid borrow conflict
        let chars_vec = self.cluster.chars().to_vec();
        let mut byte_start = self.cluster_start;
        let mut chars_iter = chars_vec.iter();

        self.selected_fonts.clear();
        self.selector
            .select_font(self.text, self.text_analysis, self.cluster, self.selected_fonts);

        for (font, num_chars) in self.selected_fonts.take() {
            if num_chars == 0 {
                continue;
            }
            let mut byte_end = byte_start;
            for _ in 0..num_chars {
                if let Some(input_char) = chars_iter.next() {
                    if let Some((off, _)) = self.text[byte_end..].char_indices().next() {
                        byte_end += off + input_char.char.len_utf8();
                    }
                }
            }
            (self.push_segment)(FontSegment {
                bidi_level: self.text_segment.bidi_level(),
                script: self.text_segment.script(),
                language: self.text_segment.language(),
                font: font.handle(),
                metrics: font.metrics(),
                byte_range: byte_start..byte_end,
            });
            byte_start = byte_end;
        }

        self.cluster.clear();
        self.char_count = 0;
        self.cluster_start = byte_start;
    }
}

impl<S: FontSelector, F: FnMut(FontSegment)> SegmentEventSink for FontEventSink<'_, S, F> {
    fn start_cluster(&mut self, _cluster: &crate::text::Cluster) {
        self.cluster.clear();
        self.cluster
            .set_context(self.text_segment.script(), self.text_segment.language());
        self.char_count = 0;
    }

    fn end_cluster(&mut self) {
        self.finish_pending_cluster();
    }

    fn element(&mut self, element: &crate::Element) {
        self.current_element = element.handle;
    }

    fn char_at(&mut self, ch: char, byte_index: usize) {
        self.cluster.push(InputChar {
            char: ch,
            element_handle: self.current_element,
        });
        self.char_count += 1;
        if self.char_count == 1 {
            self.cluster_start = byte_index;
        }
    }
}

#[cfg(all(test, feature = "icu"))]
mod tests {
    use super::*;
    #[cfg(feature = "fontique")]
    use alloc::rc::Rc;
    #[cfg(feature = "fontique")]
    use core::cell::RefCell;
    use crate::text::{
        BidiDirection, SourceElement, SourceElementKind, TextAnalysisProperties,
        TextAnalysisPropertiesProvider, TextAnalyzer,
    };

    struct FixedFontSelector {
        font: Font,
    }

    impl FontSelector for FixedFontSelector {
        fn select_font(
            &mut self,
            _text: &str,
            _analysis: &TextAnalysis,
            cluster: &mut InputCluster,
            set: &mut SelectedFontSet,
        ) {
            set.push(self.font, cluster.chars().len());
        }
    }

    struct DefaultProps;

    impl TextAnalysisPropertiesProvider for DefaultProps {
        fn text_analysis_properties(
            &mut self,
            _handle: &ElementHandle,
        ) -> TextAnalysisProperties {
            TextAnalysisProperties::default()
        }
    }

    #[test]
    fn analyze_selects_a_font_for_text_clusters() {
        let text = "ab";
        let mut text_analysis = TextAnalysis::default();
        let mut props = DefaultProps;
        TextAnalyzer::default()
            .analyze(
                text,
                BidiDirection::Auto,
                &mut props,
                [SourceElement {
                    handle: ElementHandle::default(),
                    kind: SourceElementKind::Text(text.len() as u32),
                }]
                .iter()
                .copied(),
                &mut text_analysis,
            )
            .unwrap();

        let mut selector = FixedFontSelector {
            font: Font::new(
                FontHandle {
                    id: 1,
                    ttc_index: 0,
                },
                FontMetrics {
                    ascent: 10.0,
                    descent: -2.0,
                    line_gap: 2.0,
                    units_per_em: 1000.0,
                },
            ),
        };
        let mut analyzer = FontAnalyzer::default();
        let mut font_analysis = FontAnalysis::default();
        analyzer.analyze(text, &text_analysis, &mut selector, &mut font_analysis);

        assert!(!font_analysis.segments.is_empty());
        assert_eq!(font_analysis.segments[0].font.id, 1);
    }

    #[cfg(feature = "fontique")]
    #[test]
    fn analyze_with_fontique_selector_produces_segments() {
        let text = "Hello world";
        let mut text_analysis = TextAnalysis::default();
        let mut props = DefaultProps;
        TextAnalyzer::default()
            .analyze(
                text,
                BidiDirection::Auto,
                &mut props,
                [SourceElement {
                    handle: ElementHandle::default(),
                    kind: SourceElementKind::Text(text.len() as u32),
                }]
                .iter()
                .copied(),
                &mut text_analysis,
            )
            .unwrap();

        let mut selector = FontiqueSelector::default();
        let mut analyzer = FontAnalyzer::default();
        let mut font_analysis = FontAnalysis::default();
        analyzer.analyze(text, &text_analysis, &mut selector, &mut font_analysis);

        assert!(!font_analysis.segments.is_empty());
        assert_eq!(font_analysis.segments[0].byte_range.start, 0);
        assert_eq!(font_analysis.segments.last().unwrap().byte_range.end, text.len());
    }

    #[cfg(feature = "fontique")]
    #[derive(Clone)]
    struct RecordingFontSelectionPropertiesProvider {
        seen: Rc<RefCell<Vec<u64>>>,
    }

    #[cfg(feature = "fontique")]
    impl FontSelectionPropertiesProvider for RecordingFontSelectionPropertiesProvider {
        fn font_selection_properties(
            &mut self,
            handle: &ElementHandle,
        ) -> FontSelectionProperties {
            self.seen.borrow_mut().push(handle.id);
            let mut props = FontSelectionProperties::default();
            props.weight = if handle.id == 0 {
                FontWeight::new(400.0)
            } else {
                FontWeight::new(700.0)
            };
            props
        }
    }

    #[cfg(feature = "fontique")]
    #[test]
    fn fontique_selector_uses_element_handle_properties_provider() {
        let text = "ab";
        let mut text_analysis = TextAnalysis::default();
        let mut props = DefaultProps;
        TextAnalyzer::default()
            .analyze(
                text,
                BidiDirection::Auto,
                &mut props,
                [
                    SourceElement {
                        handle: ElementHandle {
                            id: 0,
                            context_id: 0,
                        },
                        kind: SourceElementKind::Text(1),
                    },
                    SourceElement {
                        handle: ElementHandle {
                            id: 1,
                            context_id: 0,
                        },
                        kind: SourceElementKind::Text(1),
                    },
                ]
                .iter()
                .copied(),
                &mut text_analysis,
            )
            .unwrap();

        let seen = Rc::new(RefCell::new(Vec::new()));
        let provider = RecordingFontSelectionPropertiesProvider {
            seen: seen.clone(),
        };
        let mut selector = FontiqueSelector::new(provider);
        let mut analyzer = FontAnalyzer::default();
        let mut font_analysis = FontAnalysis::default();
        analyzer.analyze(text, &text_analysis, &mut selector, &mut font_analysis);

        assert!(!font_analysis.segments.is_empty());
        let seen = seen.borrow();
        assert!(seen.contains(&0));
        assert!(seen.contains(&1));
    }

    #[cfg(all(feature = "fontique", target_os = "windows"))]
    #[test]
    fn fontique_selector_uses_named_windows_families_per_element() {
        #[derive(Clone)]
        struct WindowsFamilyProvider;

        impl FontSelectionPropertiesProvider for WindowsFamilyProvider {
            fn font_selection_properties(
                &mut self,
                handle: &ElementHandle,
            ) -> FontSelectionProperties {
                let mut props = FontSelectionProperties::default();
                props.families = if handle.id == 0 {
                    vec![FontFamily::Named(String::from("Consolas"))]
                } else {
                    vec![FontFamily::Named(String::from("Arial"))]
                };
                props
            }
        }

        let text = "ab";
        let mut text_analysis = TextAnalysis::default();
        let mut props = DefaultProps;
        TextAnalyzer::default()
            .analyze(
                text,
                BidiDirection::Auto,
                &mut props,
                [
                    SourceElement {
                        handle: ElementHandle {
                            id: 0,
                            context_id: 0,
                        },
                        kind: SourceElementKind::Text(1),
                    },
                    SourceElement {
                        handle: ElementHandle {
                            id: 1,
                            context_id: 0,
                        },
                        kind: SourceElementKind::Text(1),
                    },
                ]
                .iter()
                .copied(),
                &mut text_analysis,
            )
            .unwrap();

        let mut selector = FontiqueSelector::new(WindowsFamilyProvider);
        let mut analyzer = FontAnalyzer::default();
        let mut font_analysis = FontAnalysis::default();
        analyzer.analyze(text, &text_analysis, &mut selector, &mut font_analysis);

        assert!(font_analysis.segments.len() >= 2);
        let first = &font_analysis.segments[0];
        let second = &font_analysis.segments[1];
        assert_eq!(first.byte_range, 0..1);
        assert_eq!(second.byte_range, 1..2);
        assert_ne!(first.font.id, 0);
        assert_ne!(second.font.id, 0);
        assert_ne!(first.font.id, second.font.id);
    }
}
