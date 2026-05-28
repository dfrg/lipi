use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use lipi::text::{
    BidiDirection, Cluster, Segment, SegmentEvent, SegmentEventSink, SourceElement,
    SourceElementKind, TextAnalysis, TextAnalysisProperties, TextAnalysisPropertiesProvider,
    TextAnalyzer,
};
use lipi::{Element, ElementHandle, Language};

struct PropSet<'a>(&'a [TextAnalysisProperties]);

impl TextAnalysisPropertiesProvider for PropSet<'_> {
    fn text_analysis_properties(&mut self, handle: &ElementHandle) -> TextAnalysisProperties {
        self.0[handle.id as usize]
    }
}

fn build_analysis(text: &str) -> (TextAnalysis, Vec<usize>) {
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
        .enumerate()
        .filter_map(|(i, s)| matches!(s, Segment::Text(_)).then_some(i))
        .collect::<Vec<_>>();

    (analysis, text_segments)
}

fn build_analysis_low_density(text: &str) -> (TextAnalysis, Vec<usize>) {
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
        .enumerate()
        .filter_map(|(i, s)| matches!(s, Segment::Text(_)).then_some(i))
        .collect::<Vec<_>>();

    (analysis, text_segments)
}

fn consume_iter_events(analysis: &TextAnalysis, text: &str, text_segments: &[usize]) -> usize {
    let mut acc = 0usize;
    for &segment_index in text_segments {
        if let Some(events) = analysis.segment_events(text, segment_index) {
            for event in events {
                match event {
                    SegmentEvent::StartCluster(cluster) => {
                        acc = acc.wrapping_add(cluster.text_range().start);
                    }
                    SegmentEvent::EndCluster => {
                        acc ^= 0x9E37;
                    }
                    SegmentEvent::Element(element) => {
                        acc = acc.wrapping_add(element.handle.id as usize);
                    }
                    SegmentEvent::Char(ch, byte_index) => {
                        acc = acc.wrapping_add(byte_index);
                        acc ^= ch as usize;
                    }
                }
            }
        }
    }
    acc
}

fn consume_iter_events2(analysis: &TextAnalysis, text: &str, text_segments: &[usize]) -> usize {
    let mut acc = 0usize;
    for &segment_index in text_segments {
        for event in analysis.segment_events2(text, segment_index) {
            match event {
                SegmentEvent::StartCluster(cluster) => {
                    acc = acc.wrapping_add(cluster.text_range().start);
                }
                SegmentEvent::EndCluster => {
                    acc ^= 0x9E37;
                }
                SegmentEvent::Element(element) => {
                    acc = acc.wrapping_add(element.handle.id as usize);
                }
                SegmentEvent::Char(ch, byte_index) => {
                    acc = acc.wrapping_add(byte_index);
                    acc ^= ch as usize;
                }
            }
        }
    }
    acc
}

fn consume_iter_events2_baseline(
    analysis: &TextAnalysis,
    text: &str,
    text_segments: &[usize],
) -> usize {
    let mut acc = 0usize;
    for &segment_index in text_segments {
        for event in analysis.segment_events2(text, segment_index) {
            match event {
                SegmentEvent::StartCluster(cluster) => {
                    acc = acc.wrapping_add(cluster.text_range().start);
                }
                SegmentEvent::EndCluster => {
                    acc ^= 0x9E37;
                }
                SegmentEvent::Element(element) => {
                    acc = acc.wrapping_add(element.handle.id as usize);
                }
                SegmentEvent::Char(ch, byte_index) => {
                    acc = acc.wrapping_add(byte_index);
                    acc ^= ch as usize;
                }
            }
        }
    }
    acc
}

fn consume_iter_events2_lowered(
    analysis: &TextAnalysis,
    text: &str,
    text_segments: &[usize],
) -> usize {
    let mut acc = 0usize;
    for &segment_index in text_segments {
        for event in analysis.segment_events2_lowered(text, segment_index) {
            match event {
                SegmentEvent::StartCluster(cluster) => {
                    acc = acc.wrapping_add(cluster.text_range().start);
                }
                SegmentEvent::EndCluster => {
                    acc ^= 0x9E37;
                }
                SegmentEvent::Element(element) => {
                    acc = acc.wrapping_add(element.handle.id as usize);
                }
                SegmentEvent::Char(ch, byte_index) => {
                    acc = acc.wrapping_add(byte_index);
                    acc ^= ch as usize;
                }
            }
        }
    }
    acc
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

fn walk_with_sink<S: SegmentEventSink>(
    analysis: &TextAnalysis,
    text: &str,
    text_segments: &[usize],
    sink: &mut S,
) {
    for &segment_index in text_segments {
        if let Some(events) = analysis.segment_events(text, segment_index) {
            for event in events {
                match event {
                    SegmentEvent::StartCluster(cluster) => sink.start_cluster(&cluster),
                    SegmentEvent::EndCluster => sink.end_cluster(),
                    SegmentEvent::Element(element) => sink.element(element),
                    SegmentEvent::Char(ch, byte_index) => sink.char_at(ch, byte_index),
                }
            }
        }
    }
}

fn walk_with_dyn_sink(
    analysis: &TextAnalysis,
    text: &str,
    text_segments: &[usize],
    sink: &mut dyn SegmentEventSink,
) {
    for &segment_index in text_segments {
        if let Some(events) = analysis.segment_events(text, segment_index) {
            for event in events {
                match event {
                    SegmentEvent::StartCluster(cluster) => sink.start_cluster(&cluster),
                    SegmentEvent::EndCluster => sink.end_cluster(),
                    SegmentEvent::Element(element) => sink.element(element),
                    SegmentEvent::Char(ch, byte_index) => sink.char_at(ch, byte_index),
                }
            }
        }
    }
}

fn walk_direct_with_sink<S: SegmentEventSink>(
    analysis: &TextAnalysis,
    text: &str,
    text_segments: &[usize],
    sink: &mut S,
) {
    for &segment_index in text_segments {
        analysis
            .segment_events_with(text, segment_index, sink)
            .expect("text segment event walk");
    }
}

fn walk_direct_with_dyn_sink(
    analysis: &TextAnalysis,
    text: &str,
    text_segments: &[usize],
    sink: &mut dyn SegmentEventSink,
) {
    for &segment_index in text_segments {
        analysis
            .segment_events_with(text, segment_index, sink)
            .expect("text segment event walk");
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

        group.bench_with_input(BenchmarkId::new("iterator_match", size), &size, |b, _| {
            b.iter(|| black_box(consume_iter_events(&analysis, &text, &text_segments)));
        });

        group.bench_with_input(BenchmarkId::new("iterator2_match", size), &size, |b, _| {
            b.iter(|| black_box(consume_iter_events2(&analysis, &text, &text_segments)));
        });

        group.bench_with_input(
            BenchmarkId::new("iterator2_baseline_match", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(consume_iter_events2_baseline(
                        &analysis,
                        &text,
                        &text_segments,
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("iterator2_lowered_match", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(consume_iter_events2_lowered(
                        &analysis,
                        &text,
                        &text_segments,
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("iterator2_match_low_density", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(consume_iter_events2(
                        &analysis_low,
                        &text,
                        &text_segments_low,
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("iterator2_baseline_match_low_density", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(consume_iter_events2_baseline(
                        &analysis_low,
                        &text,
                        &text_segments_low,
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("iterator2_lowered_match_low_density", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(consume_iter_events2_lowered(
                        &analysis_low,
                        &text,
                        &text_segments_low,
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("callback_direct_static_low_density", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut sink = AccSink::default();
                    walk_direct_with_sink(&analysis_low, &text, &text_segments_low, &mut sink);
                    black_box(sink.finish())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("callback_iter_static", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut sink = AccSink::default();
                    walk_with_sink(&analysis, &text, &text_segments, &mut sink);
                    black_box(sink.finish())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("callback_iter_dyn", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut sink = AccSink::default();
                    walk_with_dyn_sink(&analysis, &text, &text_segments, &mut sink);
                    black_box(sink.finish())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("callback_direct_static", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut sink = AccSink::default();
                    walk_direct_with_sink(&analysis, &text, &text_segments, &mut sink);
                    black_box(sink.finish())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("callback_direct_dyn", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut sink = AccSink::default();
                    walk_direct_with_dyn_sink(&analysis, &text, &text_segments, &mut sink);
                    black_box(sink.finish())
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_segment_events);
criterion_main!(benches);
