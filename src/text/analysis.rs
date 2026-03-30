//! Type to hold text analysis results.

use super::{
    is_real_script, BidiLevel, Cluster, ClusterAttributes, ClusterContent, ClusterRange,
    PendingCluster,
};
use crate::element::{Element, ObjectHandle};
use crate::Script;
use alloc::vec::Vec;
use core::ops::Range;

#[derive(Clone, Default)]
/// Results of text analysis.
pub struct TextAnalysis {
    /// Sequence of elements.
    pub elements: Vec<Element>,
    /// Segmentation and classification of clusters.
    pub cluster: ClusterAnalysis,
    /// Bidirectional algorithm results.
    pub bidi: BidiAnalysis,
    /// Sequence of paragraphs.
    pub paragraphs: Vec<Paragraph>,
}

impl TextAnalysis {
    /// Clears the analysis results.
    pub fn clear(&mut self) {
        self.elements.clear();
        self.cluster.clear();
        self.bidi.clear();
        self.paragraphs.clear();
    }
}

/// Results of the bidirectional algorithm.
#[derive(Clone, Default)]
pub struct BidiAnalysis {
    segments: Vec<BidiSegment>,
}

impl BidiAnalysis {
    /// Returns the underlying segments.
    pub fn segments(&self) -> &[BidiSegment] {
        &self.segments
    }

    /// Clears the analysis results.
    pub fn clear(&mut self) {
        self.segments.clear();
    }

    pub(super) fn push(&mut self, segment: BidiSegment) {
        self.segments.push(segment);
    }
}

/// Bidirectinal segmentation element
#[derive(Clone, Debug)]
pub enum BidiSegment {
    /// A text run.
    Text(BidiLevel, ClusterRange),
    /// An inline object.
    Object(BidiLevel, ObjectHandle),
}

impl BidiSegment {
    /// Returns the bidirectional level for this segment.
    pub const fn level(&self) -> BidiLevel {
        match self {
            Self::Text(level, _) | Self::Object(level, _) => *level,
        }
    }
}

/// Information about a paragraph.
#[derive(Clone, Debug)]
pub struct Paragraph {
    /// Resolved bidirectional level.
    pub level: BidiLevel,
    /// The range of text and clusters covered.
    pub range: ClusterRange,
}

/// Results of segmentation and classification of clusters.
#[derive(Clone, Default)]
pub struct ClusterAnalysis {
    pub(super) flags: Vec<ClusterAttributes>,
    pub(super) ends: Vec<u32>,
    pub(super) script_segments: Vec<ScriptBidiSegment>,
    num_objects: usize,
}

/// Fragment of text split by script and bidirectional level.
#[derive(Clone, Debug)]
pub struct ScriptBidiSegment {
    pub script: Script,
    pub range: ClusterRange,
}

impl ClusterAnalysis {
    /// Denotes that a cluster has been replaced by an object.
    const REPLACEMENT: u32 = 0x1;
}

impl ClusterAnalysis {
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
        let start = (*self.ends.get(index.saturating_sub(1))? >> 2) as usize;
        let end_with_flags = *self.ends.get(index)?;
        let is_replaced = end_with_flags & Self::REPLACEMENT != 0;
        let end = (end_with_flags >> 2) as usize;
        Some(Cluster {
            attributes: flags,
            text_range: start..end,
            is_replaced,
        })
    }

    /// Returns an iterator over the sequence of clusters.
    pub fn iter(&self) -> impl Iterator<Item = Cluster> + '_ {
        self.iter_range(&self.full_range())
    }

    /// Returns an iterator over the sequence of clusters in the given range.
    pub fn iter_range(&self, range: &ClusterRange) -> impl Iterator<Item = Cluster> + '_ {
        let mut tracking_start = range.text.start;
        let flags = self
            .flags
            .get(range.clusters.clone())
            .unwrap_or_default()
            .iter()
            .copied();
        let ends = self
            .ends
            .get(range.clusters.clone())
            .unwrap_or_default()
            .iter()
            .copied();
        flags.zip(ends).map(move |(flags, end)| {
            let start = tracking_start;
            let is_replaced = end & Self::REPLACEMENT != 0;
            let end = (end >> 2) as usize;
            tracking_start = end;
            Cluster {
                attributes: flags,
                text_range: start..end,
                is_replaced,
            }
        })
    }

    /// Clears the analysis data.
    pub fn clear(&mut self) {
        self.flags.clear();
        self.ends.clear();
        self.script_segments.clear();
        self.num_objects = 0;
    }

    fn full_range(&self) -> ClusterRange {
        let end = self
            .ends
            .last()
            .map(|pos| (*pos as usize) >> 2)
            .unwrap_or(0);
        ClusterRange {
            text: 0..end,
            clusters: 0..self.flags.len(),
        }
    }
}

impl ClusterAnalysis {
    pub(super) fn next_object(&mut self) -> ObjectHandle {
        let idx = self.num_objects;
        self.num_objects += 1;
        ObjectHandle(idx as u32)
    }

    pub(super) fn push(&mut self, cluster: &PendingCluster, is_replaced: bool) {
        println!(
            "pushing cluster with char {:?}, text {:?}, replaced: {is_replaced:}",
            cluster.base_char,
            cluster.range.clone()
        );
        if cluster.range.is_empty() {
            return;
        }
        let cluster_start = self.flags.len();
        self.flags.push(cluster.attrs);
        self.ends
            .push((cluster.range.end as u32) << 2 | is_replaced as u32);
        if !is_replaced {
            let mut next = cluster.script;
            if cluster.attrs.is_emoji_or_symbol() {
                next = match cluster.attrs.content() {
                    ClusterContent::Emoji => Script::from_bytes(*b"Zsye"),
                    _ => Script::from_bytes(*b"Zsym"),
                };
            }
            if let Some(last_script_segment) = self.script_segments.last_mut() {
                let (do_merge, script) =
                    if cluster.range.start == last_script_segment.range.text.end {
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
                    last_script_segment.range.clusters.end = self.flags.len();
                    last_script_segment.range.text.end = cluster.range.end;
                } else {
                    let cluster_end = self.flags.len();
                    self.script_segments.push(ScriptBidiSegment {
                        script,
                        range: ClusterRange {
                            text: cluster.range.clone(),
                            clusters: cluster_start..cluster_end,
                        },
                    })
                }
            } else {
                let cluster_end = self.flags.len();
                self.script_segments.push(ScriptBidiSegment {
                    script: next,
                    range: ClusterRange {
                        text: cluster.range.clone(),
                        clusters: cluster_start..cluster_end,
                    },
                })
            }
        }
    }

    pub(super) fn set_rtl(&mut self, range: &Range<usize>) {
        for cluster in self
            .flags
            .get_mut(range.clone())
            .unwrap_or_default()
            .iter_mut()
        {
            cluster.set_rtl();
        }
    }
}
