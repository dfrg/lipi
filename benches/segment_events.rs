use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use lipi::text::{
    BidiDirection, Cluster, Segment, SegmentEventSink, SourceElement, SourceElementKind,
    TextAnalysis, TextAnalysisProperties, TextAnalysisPropertiesProvider, TextAnalyzer,
    TextSegment,
};
use lipi::{Element, ElementHandle, Language};

struct PropSet<'a>(&'a [TextAnalysisProperties]);

impl TextAnalysisPropertiesProvider for PropSet<'_> {
    fn text_analysis_properties(&mut self, handle: &ElementHandle) -> TextAnalysisProperties {
        self.0[handle.id as usize]
    }
}

fn build_analysis(text: &str) -> (TextAnalysis, Vec<TextSegment>) {
    let mut elements = Vec::new();
    let mut props = Vec::new();
    let p = TextAnalysisProperties {
        language: Some(Language::parse_prefix("en").unwrap().0),
        ..Default::default()
    };

    let mut remaining = text.len();
    let mut offset = 0usize;
    let mut id = 0u64;
    while remaining > 0 {
        let take = remaining.min(24);
        elements.push(SourceElement {
            handle: ElementHandle { id, context_id: 0 },
            kind: SourceElementKind::Text(take as u32),
        });
        props.push(p);
        id += 1;

        if offset > 0 {
            elements.push(SourceElement {
                handle: ElementHandle { id, context_id: 0 },
                kind: SourceElementKind::Marker,
            });
            props.push(p);
            id += 1;

            elements.push(SourceElement {
                handle: ElementHandle { id, context_id: 0 },
                kind: SourceElementKind::StartSpan,
            });
            props.push(p);
            id += 1;
        }

        remaining -= take;
        offset += take;
    }

    let mut analyzer = TextAnalyzer::default();
    let mut analysis = TextAnalysis::default();
    let mut prop_set = PropSet(&props);
    analyzer
        .analyze(
            text,
            BidiDirection::Auto,
            &mut prop_set,
            elements.iter().copied(),
            &mut analysis,
        )
        .unwrap();

    let text_segments = analysis
        .segments
        .iter()
        .filter_map(|s| match s {
            Segment::Text(segment) => Some(segment.clone()),
            Segment::Object(_, _) => None,
        })
        .collect::<Vec<_>>();

    (analysis, text_segments)
}

fn build_analysis_low_density(text: &str) -> (TextAnalysis, Vec<TextSegment>) {
    let mut elements = Vec::new();
    let mut props = Vec::new();
    let p = TextAnalysisProperties {
        language: Some(Language::parse_prefix("en").unwrap().0),
        ..Default::default()
    };

    // Sparse boundaries: large text runs with no synthetic marker/span elements.
    let mut remaining = text.len();
    let mut id = 0u64;
    while remaining > 0 {
        let take = remaining.min(512);
        elements.push(SourceElement {
            handle: ElementHandle { id, context_id: 0 },
            kind: SourceElementKind::Text(take as u32),
        });
        props.push(p);
        id += 1;
        remaining -= take;
    }

    let mut analyzer = TextAnalyzer::default();
    let mut analysis = TextAnalysis::default();
    let mut prop_set = PropSet(&props);
    analyzer
        .analyze(
            text,
            BidiDirection::Auto,
            &mut prop_set,
            elements.iter().copied(),
            &mut analysis,
        )
        .unwrap();

    let text_segments = analysis
        .segments
        .iter()
        .filter_map(|s| match s {
            Segment::Text(segment) => Some(segment.clone()),
            Segment::Object(_, _) => None,
        })
        .collect::<Vec<_>>();

    (analysis, text_segments)
}

fn consume_segment_events(
    analysis: &TextAnalysis,
    text: &str,
    text_segments: &[TextSegment],
) -> usize {
    let mut sink = AccSink::default();
    walk_segment_events_with_sink(analysis, text, text_segments, &mut sink);
    sink.finish()
}

#[derive(Default)]
struct AccSink {
    acc: usize,
}

impl SegmentEventSink for AccSink {
    fn start_cluster(&mut self, cluster: &Cluster) {
        self.acc = self.acc.wrapping_add(cluster.text_range().start);
    }

    fn end_cluster(&mut self) {
        self.acc ^= 0x9E37;
    }

    fn element(&mut self, element: &Element) {
        self.acc = self.acc.wrapping_add(element.handle.id as usize);
    }

    fn char_at(&mut self, ch: char, byte_index: usize) {
        self.acc = self.acc.wrapping_add(byte_index);
        self.acc ^= ch as usize;
    }
}

impl AccSink {
    fn finish(&self) -> usize {
        self.acc
    }
}

fn walk_segment_events_with_sink<S: SegmentEventSink>(
    analysis: &TextAnalysis,
    text: &str,
    text_segments: &[TextSegment],
    sink: &mut S,
) {
    for segment in text_segments {
        segment.events(text, analysis, sink);
    }
}

fn walk_segment_events_with_dyn_sink(
    analysis: &TextAnalysis,
    text: &str,
    text_segments: &[TextSegment],
    sink: &mut dyn SegmentEventSink,
) {
    for segment in text_segments {
        segment.events(text, analysis, sink);
    }
}

fn bench_segment_events(c: &mut Criterion) {
    let mut group = c.benchmark_group("segment_events");

    for size in [256usize, 2048, 16384] {
        let base = "Lorem ipsum dolor sit amet, 12345, cafe, a\u{0301}, emoji: 🙂; ";
        let mut text = String::new();
        while text.len() < size {
            text.push_str(base);
        }
        text.truncate(size);

        let (analysis, text_segments) = build_analysis(&text);
        let (analysis_low, text_segments_low) = build_analysis_low_density(&text);
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("segment_events_match", size),
            &size,
            |b, _| {
                b.iter(|| black_box(consume_segment_events(&analysis, &text, &text_segments)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_match_alt", size),
            &size,
            |b, _| {
                b.iter(|| black_box(consume_segment_events(&analysis, &text, &text_segments)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_baseline_match", size),
            &size,
            |b, _| {
                b.iter(|| black_box(consume_segment_events(&analysis, &text, &text_segments)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_lowered_match", size),
            &size,
            |b, _| {
                b.iter(|| black_box(consume_segment_events(&analysis, &text, &text_segments)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_lowered_fast_match", size),
            &size,
            |b, _| {
                b.iter(|| black_box(consume_segment_events(&analysis, &text, &text_segments)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_match_low_density", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(consume_segment_events(
                        &analysis_low,
                        &text,
                        &text_segments_low,
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_baseline_match_low_density", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(consume_segment_events(
                        &analysis_low,
                        &text,
                        &text_segments_low,
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_lowered_match_low_density", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(consume_segment_events(
                        &analysis_low,
                        &text,
                        &text_segments_low,
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_lowered_fast_match_low_density", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(consume_segment_events(
                        &analysis_low,
                        &text,
                        &text_segments_low,
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_callback_static_low_density", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut sink = AccSink::default();
                    walk_segment_events_with_sink(
                        &analysis_low,
                        &text,
                        &text_segments_low,
                        &mut sink,
                    );
                    black_box(sink.finish())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_callback_static", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut sink = AccSink::default();
                    walk_segment_events_with_sink(&analysis, &text, &text_segments, &mut sink);
                    black_box(sink.finish())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_callback_dyn", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut sink = AccSink::default();
                    walk_segment_events_with_dyn_sink(&analysis, &text, &text_segments, &mut sink);
                    black_box(sink.finish())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_direct_static", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut sink = AccSink::default();
                    walk_segment_events_with_sink(&analysis, &text, &text_segments, &mut sink);
                    black_box(sink.finish())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("segment_events_direct_dyn", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut sink = AccSink::default();
                    walk_segment_events_with_dyn_sink(&analysis, &text, &text_segments, &mut sink);
                    black_box(sink.finish())
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_segment_events);
criterion_main!(benches);
