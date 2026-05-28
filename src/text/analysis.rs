//! Type to hold text analysis results.

use super::{
    bidi::{self, BidiBracket, BidiClass},
    is_real_script,
    unicode::CharProperties,
    BidiLevel,
};
use crate::element::{Element, ObjectHandle};
use crate::{range::Range32, Language, Script};
use alloc::vec::Vec;
use core::mem;
use core::ops::{Deref, DerefMut, Range};

#[derive(Clone, Default)]
/// Results of text analysis.
pub struct TextAnalysis {
    /// Sequence of elements.
    pub elements: Vec<Element>,
    /// Segmentation and classification of clusters.
    pub clusters: ClusterAnalysis,
    /// Sequence of segments.
    pub segments: Vec<Segment>,
    /// Sequence of paragraphs.
    pub paragraphs: Vec<Paragraph>,
}

impl TextAnalysis {
    /// Clears the analysis results.
    pub fn clear(&mut self) {
        self.elements.clear();
        self.clusters.clear();
        self.segments.clear();
        self.paragraphs.clear();
    }

    /// Returns an event stream for a text segment.
    pub fn segment_events<'a>(
        &'a self,
        text: &'a str,
        segment_index: usize,
    ) -> Option<impl Iterator<Item = SegmentEvent<'a>> + 'a> {
        let Segment::Text(_) = self.segments.get(segment_index)? else {
            return None;
        };
        Some(self.segment_events2_lowered(text, segment_index))
    }

    /// Visits events for a text segment using a callback sink.
    pub fn segment_events_with<S: SegmentEventSink + ?Sized>(
        &self,
        text: &str,
        segment_index: usize,
        sink: &mut S,
    ) -> Option<()> {
        let Segment::Text(segment) = self.segments.get(segment_index)? else {
            return None;
        };
        let cluster_indices = segment.clusters();
        if cluster_indices.is_empty() {
            return Some(());
        }

        let first_cluster_start = self.clusters.get(cluster_indices.start)?.text_range().start;
        let (mut element_index, mut element_range) =
            Self::next_element_for_byte(&self.elements, 0, first_cluster_start)?;

        for (_cluster_offset, cluster) in self.clusters.iter_range(cluster_indices).enumerate() {
            let range = cluster.text_range();
            if range.is_empty() {
                continue;
            }
            let cluster_slice = text.get(range.clone())?;

            if range.start >= element_range.end && element_range.start != range.start {
                let next = Self::next_element_for_byte(&self.elements, element_index, range.start)?;
                element_index = next.0;
                element_range = next.1;
            }

            while element_range.start == range.start {
                sink.element(&self.elements[element_index]);
                if let Some((next_index, next_range)) =
                    Self::next_element_for_byte(&self.elements, element_index + 1, range.start)
                {
                    if next_range.start == range.start {
                        element_index = next_index;
                        element_range = next_range;
                        continue;
                    }
                }
                break;
            }

            sink.start_cluster(&cluster);
            for (local_byte_index, ch) in cluster_slice.char_indices() {
                let byte_index = range.start + local_byte_index;

                if byte_index >= element_range.end && element_range.start != byte_index {
                    let next =
                        Self::next_element_for_byte(&self.elements, element_index, byte_index)?;
                    element_index = next.0;
                    element_range = next.1;
                }

                while element_range.start == byte_index && byte_index != range.start {
                    sink.element(&self.elements[element_index]);
                    if let Some((next_index, next_range)) =
                        Self::next_element_for_byte(&self.elements, element_index + 1, byte_index)
                    {
                        if next_range.start == byte_index {
                            element_index = next_index;
                            element_range = next_range;
                            continue;
                        }
                    }
                    break;
                }

                sink.char_at(ch, byte_index);
            }
            sink.end_cluster();
        }

        Some(())
    }

    /// Baseline iterator implementation retained for A/B perf comparison.
    pub fn segment_events2<'a>(
        &'a self,
        text: &'a str,
        segment_index: usize,
    ) -> impl Iterator<Item = SegmentEvent<'a>> + 'a {
        let cluster_range = match self.segments.get(segment_index) {
            Some(Segment::Text(segment)) => segment.clusters(),
            _ => 0..0,
        };

        let mut clusters = self.clusters.iter_range(cluster_range.clone());
        let mut element_index = 0usize;
        let mut element_range = 0..0;

        #[derive(Clone)]
        struct Cursor<'a> {
            cluster: Cluster,
            start: usize,
            end: usize,
            chars: core::str::CharIndices<'a>,
        }

        #[derive(Copy, Clone)]
        struct Pending {
            byte_index: usize,
            ch: char,
            ends_cluster: bool,
        }

        enum State<'a> {
            NeedCluster,
            PreStartElements(Cursor<'a>),
            Start(Cursor<'a>),
            FastChars(Cursor<'a>),
            NextChar(Cursor<'a>),
            EmitElement(Cursor<'a>, Pending),
            EmitChar(Cursor<'a>, Pending),
            EmitEnd,
            Done,
        }

        let mut state = if let Some(first_cluster_start) = self
            .clusters
            .get(cluster_range.start)
            .map(|c| c.text_range().start)
        {
            if let Some((ix, range)) =
                Self::next_element_for_byte(&self.elements, 0, first_cluster_start)
            {
                element_index = ix;
                element_range = range;
                State::NeedCluster
            } else {
                State::Done
            }
        } else {
            State::Done
        };

        let elements = &self.elements;
        core::iter::from_fn(move || loop {
            match mem::replace(&mut state, State::Done) {
                State::Done => {
                    state = State::Done;
                    return None;
                }
                State::NeedCluster => {
                    let Some(cluster) = clusters.next() else {
                        state = State::Done;
                        return None;
                    };
                    let range = cluster.text_range();
                    if range.is_empty() {
                        state = State::NeedCluster;
                        continue;
                    }
                    let Some(cluster_slice) = text.get(range.clone()) else {
                        state = State::Done;
                        return None;
                    };

                    let Some((ix, next_range)) =
                        Self::next_element_for_byte(elements, element_index, range.start)
                    else {
                        state = State::Done;
                        return None;
                    };
                    element_index = ix;
                    element_range = next_range;

                    state = State::PreStartElements(Cursor {
                        cluster,
                        start: range.start,
                        end: range.end,
                        chars: cluster_slice.char_indices(),
                    });
                    continue;
                }
                State::PreStartElements(cursor) => {
                    if element_range.start != cursor.start {
                        state = State::Start(cursor);
                        continue;
                    }

                    let event = SegmentEvent::Element(&elements[element_index]);
                    if let Some((ix, next_range)) =
                        Self::next_element_for_byte(elements, element_index + 1, cursor.start)
                    {
                        if next_range.start == cursor.start {
                            element_index = ix;
                            element_range = next_range;
                            state = State::PreStartElements(cursor);
                        } else {
                            element_index = ix;
                            element_range = next_range;
                            state = State::Start(cursor);
                        }
                    } else {
                        state = State::Start(cursor);
                    }
                    return Some(event);
                }
                State::Start(cursor) => {
                    let event = SegmentEvent::StartCluster(cursor.cluster.clone());
                    let has_interior_element_start = elements
                        .get(element_index + 1)
                        .map(|next| next.text_range().start < cursor.end)
                        .unwrap_or(false);
                    state = if has_interior_element_start {
                        State::NextChar(cursor)
                    } else {
                        State::FastChars(cursor)
                    };
                    return Some(event);
                }
                State::FastChars(mut cursor) => {
                    let Some((local_byte_index, ch)) = cursor.chars.next() else {
                        state = State::EmitEnd;
                        continue;
                    };
                    let byte_index = cursor.start + local_byte_index;
                    state = State::FastChars(cursor);
                    return Some(SegmentEvent::Char(ch, byte_index));
                }
                State::NextChar(mut cursor) => {
                    let Some((local_byte_index, ch)) = cursor.chars.next() else {
                        state = State::EmitEnd;
                        continue;
                    };

                    let byte_index = cursor.start + local_byte_index;
                    let pending = Pending {
                        byte_index,
                        ch,
                        ends_cluster: byte_index + ch.len_utf8() == cursor.end,
                    };

                    let Some((ix, next_range)) =
                        Self::next_element_for_byte(elements, element_index, byte_index)
                    else {
                        state = State::Done;
                        return None;
                    };
                    element_index = ix;
                    element_range = next_range;

                    if byte_index == element_range.start && byte_index != cursor.start {
                        state = State::EmitElement(cursor, pending);
                        continue;
                    }

                    state = State::EmitChar(cursor, pending);
                    continue;
                }
                State::EmitElement(cursor, pending) => {
                    let event = SegmentEvent::Element(&elements[element_index]);
                    if let Some((ix, next_range)) =
                        Self::next_element_for_byte(elements, element_index + 1, pending.byte_index)
                    {
                        element_index = ix;
                        element_range = next_range;
                        if element_range.start == pending.byte_index {
                            state = State::EmitElement(cursor, pending);
                        } else {
                            state = State::EmitChar(cursor, pending);
                        }
                    } else {
                        state = State::EmitChar(cursor, pending);
                    }
                    return Some(event);
                }
                State::EmitChar(cursor, pending) => {
                    let event = SegmentEvent::Char(pending.ch, pending.byte_index);
                    state = if pending.ends_cluster {
                        State::EmitEnd
                    } else {
                        State::NextChar(cursor)
                    };
                    return Some(event);
                }
                State::EmitEnd => {
                    state = State::NeedCluster;
                    return Some(SegmentEvent::EndCluster);
                }
            }
        })
    }

    /// Mechanically lowered iterator that tracks the callback walk closely.
    pub fn segment_events2_lowered<'a>(
        &'a self,
        text: &'a str,
        segment_index: usize,
    ) -> impl Iterator<Item = SegmentEvent<'a>> + 'a {
        let cluster_range = match self.segments.get(segment_index) {
            Some(Segment::Text(segment)) => segment.clusters(),
            _ => 0..0,
        };

        let mut clusters = self.clusters.iter_range(cluster_range.clone());
        let mut element_index = 0usize;
        let mut element_range = 0..0;

        struct Cursor<'a> {
            cluster: Cluster,
            start: usize,
            chars: core::str::CharIndices<'a>,
        }

        #[derive(Copy, Clone)]
        struct Pending {
            byte_index: usize,
            ch: char,
        }

        enum State<'a> {
            NeedCluster,
            StartElements(Cursor<'a>),
            StartCluster(Cursor<'a>),
            NextChar(Cursor<'a>),
            CharElements(Cursor<'a>, Pending),
            EmitChar(Cursor<'a>, Pending),
            EndCluster,
            Done,
        }

        let mut state = if let Some(first_cluster_start) = self
            .clusters
            .get(cluster_range.start)
            .map(|c| c.text_range().start)
        {
            if let Some((ix, range)) =
                Self::next_element_for_byte(&self.elements, 0, first_cluster_start)
            {
                element_index = ix;
                element_range = range;
                State::NeedCluster
            } else {
                State::Done
            }
        } else {
            State::Done
        };

        let elements = &self.elements;
        core::iter::from_fn(move || loop {
            match mem::replace(&mut state, State::Done) {
                State::Done => {
                    state = State::Done;
                    return None;
                }
                State::NeedCluster => {
                    let Some(cluster) = clusters.next() else {
                        state = State::Done;
                        return None;
                    };
                    let range = cluster.text_range();
                    if range.is_empty() {
                        state = State::NeedCluster;
                        continue;
                    }
                    let Some(cluster_slice) = text.get(range.clone()) else {
                        state = State::Done;
                        return None;
                    };

                    if range.start >= element_range.end && element_range.start != range.start {
                        let Some((ix, next_range)) =
                            Self::next_element_for_byte(elements, element_index, range.start)
                        else {
                            state = State::Done;
                            return None;
                        };
                        element_index = ix;
                        element_range = next_range;
                    }

                    let cursor = Cursor {
                        cluster,
                        start: range.start,
                        chars: cluster_slice.char_indices(),
                    };

                    if element_range.start == cursor.start {
                        state = State::StartElements(cursor);
                        continue;
                    }

                    let event = SegmentEvent::StartCluster(cursor.cluster.clone());
                    state = State::NextChar(cursor);
                    return Some(event);
                }
                State::StartElements(cursor) => {
                    if element_range.start != cursor.start {
                        let event = SegmentEvent::StartCluster(cursor.cluster.clone());
                        state = State::NextChar(cursor);
                        return Some(event);
                    }

                    let event = SegmentEvent::Element(&elements[element_index]);
                    if let Some((next_index, next_range)) =
                        Self::next_element_for_byte(elements, element_index + 1, cursor.start)
                    {
                        if next_range.start == cursor.start {
                            element_index = next_index;
                            element_range = next_range;
                            state = State::StartElements(cursor);
                        } else {
                            element_index = next_index;
                            element_range = next_range;
                            state = State::StartCluster(cursor);
                        }
                    } else {
                        state = State::StartCluster(cursor);
                    }
                    return Some(event);
                }
                State::StartCluster(cursor) => {
                    let event = SegmentEvent::StartCluster(cursor.cluster.clone());
                    state = State::NextChar(cursor);
                    return Some(event);
                }
                State::NextChar(mut cursor) => {
                    let Some((local_byte_index, ch)) = cursor.chars.next() else {
                        state = State::EndCluster;
                        continue;
                    };

                    let byte_index = cursor.start + local_byte_index;
                    let pending = Pending { byte_index, ch };

                    if byte_index >= element_range.end && element_range.start != byte_index {
                        let Some((ix, next_range)) =
                            Self::next_element_for_byte(elements, element_index, byte_index)
                        else {
                            state = State::Done;
                            return None;
                        };
                        element_index = ix;
                        element_range = next_range;
                    }

                    state = if element_range.start == byte_index && byte_index != cursor.start {
                        State::CharElements(cursor, pending)
                    } else {
                        State::EmitChar(cursor, pending)
                    };
                    continue;
                }
                State::CharElements(cursor, pending) => {
                    if element_range.start != pending.byte_index {
                        state = State::EmitChar(cursor, pending);
                        continue;
                    }

                    let event = SegmentEvent::Element(&elements[element_index]);
                    if let Some((next_index, next_range)) =
                        Self::next_element_for_byte(elements, element_index + 1, pending.byte_index)
                    {
                        if next_range.start == pending.byte_index {
                            element_index = next_index;
                            element_range = next_range;
                            state = State::CharElements(cursor, pending);
                        } else {
                            element_index = next_index;
                            element_range = next_range;
                            state = State::EmitChar(cursor, pending);
                        }
                    } else {
                        state = State::EmitChar(cursor, pending);
                    }
                    return Some(event);
                }
                State::EmitChar(cursor, pending) => {
                    let event = SegmentEvent::Char(pending.ch, pending.byte_index);
                    state = if cursor.chars.as_str().is_empty() {
                        State::EndCluster
                    } else {
                        State::NextChar(cursor)
                    };
                    return Some(event);
                }
                State::EndCluster => {
                    state = State::NeedCluster;
                    return Some(SegmentEvent::EndCluster);
                }
            }
        })
    }

    /// Reconstructed lowered iterator from the prior fast benchmark run.
    pub fn segment_events2_lowered_fast<'a>(
        &'a self,
        text: &'a str,
        segment_index: usize,
    ) -> impl Iterator<Item = SegmentEvent<'a>> + 'a {
        let cluster_range = match self.segments.get(segment_index) {
            Some(Segment::Text(segment)) => segment.clusters(),
            _ => 0..0,
        };

        let mut clusters = self.clusters.iter_range(cluster_range.clone());
        let mut element_index = 0usize;
        let mut element_range = 0..0;

        #[derive(Clone)]
        struct Cursor<'a> {
            cluster: Cluster,
            start: usize,
            end: usize,
            chars: core::str::CharIndices<'a>,
        }

        #[derive(Copy, Clone)]
        struct Pending {
            byte_index: usize,
            ch: char,
            ends_cluster: bool,
        }

        enum State<'a> {
            NeedCluster,
            StartElements(Cursor<'a>),
            StartCluster(Cursor<'a>),
            NextChar(Cursor<'a>),
            CharElements(Cursor<'a>, Pending),
            EmitChar(Cursor<'a>, Pending),
            EndCluster,
            Done,
        }

        let mut state = if let Some(first_cluster_start) =
            self.clusters.get(cluster_range.start).map(|c| c.text_range().start)
        {
            if let Some((ix, range)) = Self::next_element_for_byte(&self.elements, 0, first_cluster_start)
            {
                element_index = ix;
                element_range = range;
                State::NeedCluster
            } else {
                State::Done
            }
        } else {
            State::Done
        };

        let elements = &self.elements;
        core::iter::from_fn(move || {
            loop {
                match mem::replace(&mut state, State::Done) {
                    State::Done => {
                        state = State::Done;
                        return None;
                    }
                    State::NeedCluster => {
                        let Some(cluster) = clusters.next() else {
                            state = State::Done;
                            return None;
                        };
                        let range = cluster.text_range();
                        if range.is_empty() {
                            state = State::NeedCluster;
                            continue;
                        }
                        let Some(cluster_slice) = text.get(range.clone()) else {
                            state = State::Done;
                            return None;
                        };

                        if range.start >= element_range.end && element_range.start != range.start {
                            let Some((ix, next_range)) =
                                Self::next_element_for_byte(elements, element_index, range.start)
                            else {
                                state = State::Done;
                                return None;
                            };
                            element_index = ix;
                            element_range = next_range;
                        }

                        let cursor = Cursor {
                            cluster,
                            start: range.start,
                            end: range.end,
                            chars: cluster_slice.char_indices(),
                        };

                        state = if element_range.start == cursor.start {
                            State::StartElements(cursor)
                        } else {
                            State::StartCluster(cursor)
                        };
                        continue;
                    }
                    State::StartElements(cursor) => {
                        if element_range.start != cursor.start {
                            state = State::StartCluster(cursor);
                            continue;
                        }

                        let event = SegmentEvent::Element(&elements[element_index]);
                        if let Some((next_index, next_range)) =
                            Self::next_element_for_byte(elements, element_index + 1, cursor.start)
                        {
                            if next_range.start == cursor.start {
                                element_index = next_index;
                                element_range = next_range;
                                state = State::StartElements(cursor);
                            } else {
                                element_index = next_index;
                                element_range = next_range;
                                state = State::StartCluster(cursor);
                            }
                        } else {
                            state = State::StartCluster(cursor);
                        }
                        return Some(event);
                    }
                    State::StartCluster(cursor) => {
                        state = State::NextChar(cursor.clone());
                        return Some(SegmentEvent::StartCluster(cursor.cluster));
                    }
                    State::NextChar(mut cursor) => {
                        let Some((local_byte_index, ch)) = cursor.chars.next() else {
                            state = State::EndCluster;
                            continue;
                        };

                        let byte_index = cursor.start + local_byte_index;
                        let pending = Pending {
                            byte_index,
                            ch,
                            ends_cluster: byte_index + ch.len_utf8() == cursor.end,
                        };

                        if byte_index >= element_range.end && element_range.start != byte_index {
                            let Some((ix, next_range)) =
                                Self::next_element_for_byte(elements, element_index, byte_index)
                            else {
                                state = State::Done;
                                return None;
                            };
                            element_index = ix;
                            element_range = next_range;
                        }

                        state = if element_range.start == byte_index && byte_index != cursor.start {
                            State::CharElements(cursor, pending)
                        } else {
                            State::EmitChar(cursor, pending)
                        };
                        continue;
                    }
                    State::CharElements(cursor, pending) => {
                        if element_range.start != pending.byte_index {
                            state = State::EmitChar(cursor, pending);
                            continue;
                        }

                        let event = SegmentEvent::Element(&elements[element_index]);
                        if let Some((next_index, next_range)) =
                            Self::next_element_for_byte(elements, element_index + 1, pending.byte_index)
                        {
                            if next_range.start == pending.byte_index {
                                element_index = next_index;
                                element_range = next_range;
                                state = State::CharElements(cursor, pending);
                            } else {
                                element_index = next_index;
                                element_range = next_range;
                                state = State::EmitChar(cursor, pending);
                            }
                        } else {
                            state = State::EmitChar(cursor, pending);
                        }
                        return Some(event);
                    }
                    State::EmitChar(cursor, pending) => {
                        let event = SegmentEvent::Char(pending.ch, pending.byte_index);
                        state = if pending.ends_cluster {
                            State::EndCluster
                        } else {
                            State::NextChar(cursor)
                        };
                        return Some(event);
                    }
                    State::EndCluster => {
                        state = State::NeedCluster;
                        return Some(SegmentEvent::EndCluster);
                    }
                }
            }
        })
    }

    fn next_element_for_byte(
        elements: &[Element],
        mut from: usize,
        byte_index: usize,
    ) -> Option<(usize, Range<usize>)> {
        while let Some(element) = elements.get(from) {
            let range = element.text_range();
            if range.start > byte_index {
                return None;
            }
            if range.start == byte_index || range.end > byte_index {
                return Some((from, range));
            }
            from += 1;
        }
        None
    }
}

/// Callback sink for segment event traversal.
pub trait SegmentEventSink {
    fn start_cluster(&mut self, cluster: &Cluster);
    fn end_cluster(&mut self);
    fn element(&mut self, element: &Element);
    fn char_at(&mut self, ch: char, byte_index: usize);
}

/// Event view over segment text processing.
#[derive(Clone, Debug)]
pub enum SegmentEvent<'a> {
    /// Start of a cluster.
    StartCluster(Cluster),
    /// End of a cluster.
    EndCluster,
    /// Current text element for subsequent chars.
    Element(&'a Element),
    /// Character payload.
    Char(char, usize),
}

/// Results of segmentation and classification of clusters.
#[derive(Clone, Default)]
pub struct ClusterAnalysis {
    pub(super) flags: Vec<ClusterAttributes>,
    pub(super) ends: Vec<u32>,
}

/// Iterator over a range of analyzed clusters.
pub struct ClusterRangeIter<'a> {
    tracking_start: usize,
    flags: core::slice::Iter<'a, ClusterAttributes>,
    ends: core::slice::Iter<'a, u32>,
}

impl Iterator for ClusterRangeIter<'_> {
    type Item = Cluster;

    fn next(&mut self) -> Option<Self::Item> {
        let flags = *self.flags.next()?;
        let end = *self.ends.next()?;
        let start = self.tracking_start;
        let text_end = (end >> ClusterAnalysis::TEXT_END_SHIFT) as usize;
        let needs_bidi_neutral_reset = (end & ClusterAnalysis::NEUTRAL_RESET_BIT) != 0;
        self.tracking_start = text_end;
        Some(Cluster {
            attributes: flags,
            text_range: Range32::from_usize(start..text_end),
            needs_bidi_neutral_reset,
        })
    }
}

impl ClusterAnalysis {
    const TEXT_END_SHIFT: u32 = 2;
    const NEUTRAL_RESET_BIT: u32 = 0b01;

    /// Returns true if the cluster sequence is empty.
    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }

    /// Returns the number of clusters.
    pub fn len(&self) -> usize {
        self.flags.len()
    }

    /// Returns the cluster at the given index.
    pub fn get(&self, index: usize) -> Option<Cluster> {
        let flags = *self.flags.get(index)?;
        let start = self.text_start(index)?;
        let end_with_flags = *self.ends.get(index)?;
        let end = (end_with_flags >> Self::TEXT_END_SHIFT) as usize;
        let needs_bidi_neutral_reset = (end_with_flags & Self::NEUTRAL_RESET_BIT) != 0;
        Some(Cluster {
            attributes: flags,
            text_range: Range32::from_usize(start..end),
            needs_bidi_neutral_reset,
        })
    }

    /// Returns an iterator over the sequence of clusters.
    pub fn iter(&self) -> ClusterRangeIter<'_> {
        self.iter_range(0..self.flags.len())
    }

    /// Returns an iterator over the sequence of clusters in the given range.
    pub fn iter_range(&self, range: Range<usize>) -> ClusterRangeIter<'_> {
        ClusterRangeIter {
            tracking_start: self.text_range(range.clone()).map(|r| r.start).unwrap_or(0),
            flags: self.flags.get(range.clone()).unwrap_or_default().iter(),
            ends: self.ends.get(range).unwrap_or_default().iter(),
        }
    }

    /// Returns the text range for the given range of clusters.
    pub fn text_range(&self, clusters: Range<usize>) -> Option<Range<usize>> {
        let text_start = self.text_start(clusters.start)?;
        if clusters.is_empty() {
            Some(text_start..text_start)
        } else {
            let text_end =
                (*self.ends.get(clusters.end.saturating_sub(1))? as usize) >> Self::TEXT_END_SHIFT;
            Some(text_start..text_end)
        }
    }

    /// Clears the analysis data.
    pub fn clear(&mut self) {
        self.flags.clear();
        self.ends.clear();
    }

    fn text_start(&self, index: usize) -> Option<usize> {
        let start = if index == 0 {
            0
        } else {
            (*self.ends.get(index - 1)? as usize) >> Self::TEXT_END_SHIFT
        };
        Some(start)
    }

    pub(super) fn push(&mut self, cluster: &PendingCluster) {
        self.flags.push(cluster.attrs);
        let mut end = cluster.range.end << Self::TEXT_END_SHIFT;
        if bidi::needs_trailing_neutral_reset(cluster.bidi_class) {
            end |= Self::NEUTRAL_RESET_BIT;
        }
        self.ends.push(end);
    }

    pub(super) fn set_rtl(&mut self, clusters: &Range<usize>) {
        for cluster in self
            .flags
            .get_mut(clusters.clone())
            .unwrap_or_default()
            .iter_mut()
        {
            cluster.set_rtl();
        }
    }
}

/// A grapheme cluster.
#[derive(Clone, Debug)]
pub struct Cluster {
    /// Cluster properties.
    attributes: ClusterAttributes,
    /// Range in the source text.
    text_range: Range32,
    /// Whether this cluster should have neutral bidi state reset to paragraph
    /// base level before visual reordering.
    needs_bidi_neutral_reset: bool,
}

impl Cluster {
    /// Returns the cluster properties.
    pub const fn attributes(&self) -> ClusterAttributes {
        self.attributes
    }

    /// Returns the range in the source text.
    pub fn text_range(&self) -> Range<usize> {
        self.text_range.to_usize()
    }

    /// Returns whether this cluster needs neutral bidi reset before
    /// visual reordering.
    pub const fn needs_bidi_neutral_reset(&self) -> bool {
        self.needs_bidi_neutral_reset
    }
}

impl Deref for Cluster {
    type Target = ClusterAttributes;

    fn deref(&self) -> &Self::Target {
        &self.attributes
    }
}

impl DerefMut for Cluster {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.attributes
    }
}

/// The content and segmentation state of a cluster.
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug)]
#[repr(transparent)]
pub struct ClusterAttributes(u8);

impl ClusterAttributes {
    /// Bits used for the content type.
    const CONTENT_MASK: u8 = 0b1111;

    /// Bit that stores line break state.
    const LINE_BREAK_BIT: u8 = 0b0001_0000;

    /// Bits used for the word type.
    const WORD_KIND_MASK: u8 = 0b0110_0000;

    /// Bit shift for the word type.
    const WORD_KIND_SHIFT: u8 = Self::WORD_KIND_MASK.trailing_zeros() as u8;

    /// Bit used to signify a right to left ordered cluster.
    const RTL_BIT: u8 = 0b1000_0000;

    /// Returns the content type of the cluster.
    pub const fn content(self) -> ClusterContent {
        ClusterContent::from_bits(self.0 & Self::CONTENT_MASK)
    }

    /// Returns true if there is a line break opportunity _after_ this cluster.
    pub const fn can_break_line_after(self) -> bool {
        self.0 & Self::LINE_BREAK_BIT != 0
    }

    /// Returns true if this cluster is the end of a word.
    pub const fn is_end_of_word(self) -> bool {
        self.word_bits() != 0
    }

    /// Returns a word kind if this cluster represents the _end_ of a word.
    pub const fn word_kind(self) -> Option<WordKind> {
        match self.word_bits() {
            0 => None,
            1 => Some(WordKind::Letter),
            2 => Some(WordKind::Number),
            3 => Some(WordKind::Other),
            _ => None,
        }
    }

    /// Returns true if this cluster is ordered left to right.
    pub const fn is_ltr(self) -> bool {
        !self.is_rtl()
    }

    /// Returns true if this cluster is ordered right to left.
    pub const fn is_rtl(self) -> bool {
        self.0 & Self::RTL_BIT != 0
    }

    /// Returns true if this cluster is an emoji or symbol.
    pub const fn is_emoji_or_symbol(self) -> bool {
        let content = self.0 & Self::CONTENT_MASK;
        content == ClusterContent::Emoji as _ || content == ClusterContent::Symbol as _
    }

    /// Returns true if this cluster is an emoji.
    pub const fn is_emoji(self) -> bool {
        (self.0 & Self::CONTENT_MASK) == ClusterContent::Emoji as _
    }

    /// Returns true if this cluster is a symbol or emoji with text
    /// presentation.
    pub const fn is_symbol(self) -> bool {
        (self.0 & Self::CONTENT_MASK) == ClusterContent::Symbol as _
    }

    /// Returns true if this cluster is any whitespace.
    pub const fn is_whitespace(self) -> bool {
        (self.0 & Self::CONTENT_MASK) >= ClusterContent::Space as _
    }

    /// Returns true if this cluster is a paragraph separator.
    pub const fn is_paragraph_separator(self) -> bool {
        (self.0 & Self::CONTENT_MASK) == ClusterContent::ParagraphSeparator as _
    }

    const fn word_bits(self) -> u8 {
        ((self.0 & Self::WORD_KIND_MASK) >> Self::WORD_KIND_SHIFT) & 0b11
    }

    pub(super) fn set_content(&mut self, content: ClusterContent) {
        self.0 = self.0 & !Self::CONTENT_MASK | (content as u8);
    }

    pub(super) fn set_line_break(&mut self) {
        self.0 |= Self::LINE_BREAK_BIT;
    }

    pub(super) fn set_word_kind(&mut self, kind: WordKind) {
        self.0 = self.0 & !Self::WORD_KIND_MASK | ((kind as u8 + 1) << Self::WORD_KIND_SHIFT);
    }

    pub(super) fn set_rtl(&mut self) {
        self.0 |= Self::RTL_BIT;
    }

    pub(super) fn new(ch: char, char_props: CharProperties) -> Self {
        let mut cluster = Self::default();
        cluster.set_content(if super::is_paragraph_separator(ch) {
            ClusterContent::ParagraphSeparator
        } else if char_props.is_extended_pictographic {
            if char_props.is_emoji_presentation {
                ClusterContent::Emoji
            } else {
                ClusterContent::Symbol
            }
        } else if char_props.is_regional_indicator {
            ClusterContent::RegionalIndicator
        } else if ch == ' ' {
            ClusterContent::Space
        } else if ch == '\u{00A0}' {
            ClusterContent::NoBreakSpace
        } else if ch == '\t' {
            ClusterContent::Tab
        } else if ch.is_whitespace() {
            ClusterContent::OtherWhitespace
        } else {
            ClusterContent::Text
        });
        cluster
    }

    pub(super) fn update_content(&mut self, ch: char) {
        const EMOJI_PRESENTATION: char = '\u{FE0F}';
        const TEXT_PRESENTATION: char = '\u{FE0E}';
        if self.is_emoji_or_symbol() {
            if ch == EMOJI_PRESENTATION {
                self.set_content(ClusterContent::Emoji);
            } else if ch == TEXT_PRESENTATION {
                self.set_content(ClusterContent::Symbol);
            }
        }
    }
}

/// The content of a cluster.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
#[repr(u8)]
pub enum ClusterContent {
    // Basic text.
    #[default]
    Text = 0,
    /// Emoji with emoji presentation.
    Emoji = 1,
    /// Symbol or emoji with text presentation.
    Symbol = 2,
    /// Regional indicators or flag emojis.
    RegionalIndicator = 3,
    /// Basic space.
    Space = 4,
    /// Non-breaking space.
    NoBreakSpace = 5,
    /// Horizontal tab.
    Tab = 6,
    /// Any newline sequence.
    ParagraphSeparator = 7,
    /// Other whitespace.
    OtherWhitespace = 8,
}

impl ClusterContent {
    const fn from_bits(bits: u8) -> Self {
        match bits {
            0 => Self::Text,
            1 => Self::Emoji,
            2 => Self::Symbol,
            3 => Self::RegionalIndicator,
            4 => Self::Space,
            5 => Self::NoBreakSpace,
            6 => Self::Tab,
            7 => Self::ParagraphSeparator,
            8 => Self::OtherWhitespace,
            _ => Self::Text,
        }
    }
}

/// The type of a word.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum WordKind {
    /// Letter like word.
    Letter = 0,
    /// Number like word.
    Number = 1,
    /// Other type of word.
    Other = 2,
}

/// A text segment.
#[derive(Clone, Debug)]
pub struct TextSegment {
    /// The Unicode script.
    pub(super) script: Script,
    /// The user specified language.
    pub(super) language: Option<Language>,
    /// The resolved bidirectional level.
    pub(super) bidi_level: BidiLevel,
    /// The cluster range for the text.
    pub(crate) clusters: Range32,
}

impl TextSegment {
    /// Creates a text segment.
    pub(super) fn new(
        script: Script,
        language: Option<Language>,
        bidi_level: BidiLevel,
        clusters: Range<usize>,
    ) -> Self {
        Self {
            script,
            language,
            bidi_level,
            clusters: Range32::from_usize(clusters),
        }
    }

    /// Returns the Unicode script.
    pub const fn script(&self) -> Script {
        self.script
    }

    /// Returns the user specified language.
    pub const fn language(&self) -> Option<Language> {
        self.language
    }

    /// Returns the resolved bidirectional level.
    pub const fn bidi_level(&self) -> BidiLevel {
        self.bidi_level
    }

    /// Returns the cluster range for the text.
    pub fn clusters(&self) -> Range<usize> {
        self.clusters.to_usize()
    }
}

/// A segment in some analyzed text.
#[derive(Clone, Debug)]
pub enum Segment {
    /// A text segment.
    Text(TextSegment),
    /// An inline object.
    Object(BidiLevel, ObjectHandle),
}

impl Segment {
    /// Returns the bidirectional level for this segment.
    pub const fn bidi_level(&self) -> BidiLevel {
        match self {
            Self::Text(segment) => segment.bidi_level,
            Self::Object(level, _) => *level,
        }
    }
}

/// Information about a paragraph.
#[derive(Clone, Debug)]
pub struct Paragraph {
    /// Resolved bidirectional level.
    pub(super) bidi_level: BidiLevel,
    /// The range of segments covered.
    segments: Range32,
}

impl Paragraph {
    /// Creates a paragraph.
    pub(super) fn new(bidi_level: BidiLevel, segments: Range<usize>) -> Self {
        Self {
            bidi_level,
            segments: Range32::from_usize(segments),
        }
    }

    /// Returns the resolved bidirectional level.
    pub const fn bidi_level(&self) -> BidiLevel {
        self.bidi_level
    }

    /// Returns the range of covered segments.
    pub fn segments(&self) -> Range<usize> {
        self.segments.to_usize()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct PendingCluster {
    pub(super) attrs: ClusterAttributes,
    pub(super) range: Range32,
    pub(super) base_char: char,
    pub(super) bidi_class: BidiClass,
    pub(super) bidi_bracket: Option<BidiBracket>,
    pub(super) script: Script,
    pub(super) lang: Option<Language>,
}

impl PendingCluster {
    pub(super) fn reset(
        &mut self,
        ch: char,
        char_props: CharProperties,
        language: Option<Language>,
        start: usize,
    ) {
        self.range.start = start as u32;
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
