//! Type to hold text analysis results.

use super::{is_real_script, ClusterContent, ClusterFlags, PendingCluster};
use crate::element::{Element, ObjectHandle};
use crate::Script;
use alloc::vec::Vec;
use core::ops::Range;

/// A synchronized range for text and clusters.
#[derive(Clone, Default, Debug)]
pub struct ClusterRange {
    /// The range in the source text in code units.
    pub text: Range<usize>,
    /// The range in the cluster buffer.
    pub clusters: Range<usize>,
}

/// A grapheme cluster.
#[derive(Clone, Debug)]
pub struct Cluster {
    /// Cluster properties.
    pub flags: ClusterFlags,
    /// Range in the source text.
    pub text_range: Range<usize>,
    /// True if this cluster was replaced.
    pub is_replaced: bool,
}

/// Results of text analysis.
#[derive(Clone, Default)]
pub struct TextAnalysis {
    pub(super) cluster_flags: Vec<ClusterFlags>,
    pub(super) cluster_ends: Vec<u32>,
    elements: Vec<Element>,
    pub(super) script_segments: Vec<ScriptBidiSegment>,
    num_objects: usize,
}

/// Fragment of text split by script and bidirectional level.
#[derive(Clone, Debug)]
pub struct ScriptBidiSegment {
    pub script: Script,
    pub range: ClusterRange,
}

impl TextAnalysis {
    /// Denotes that a cluster has been replaced by an object.
    const REPLACEMENT: u8 = 0x1;
}

impl TextAnalysis {
    /// Clears the analysis data.
    pub fn clear(&mut self) {
        self.cluster_flags.clear();
        self.cluster_ends.clear();
        self.elements.clear();
        self.script_segments.clear();
        self.num_objects = 0;
    }

    pub fn clusters_for_range(&self, range: &ClusterRange) -> impl Iterator<Item = Cluster> + '_ {
        let mut tracking_start = range.text.start;
        let flags = self
            .cluster_flags
            .get(range.clusters.clone())
            .unwrap_or_default()
            .iter()
            .copied();
        let ends = self
            .cluster_ends
            .get(range.clusters.clone())
            .unwrap_or_default()
            .iter()
            .copied();
        flags.zip(ends).map(move |(flags, end)| {
            let end = end as usize;
            let start = tracking_start;
            let is_replaced = end & 0x1 != 0;
            let end = end >> 2;
            tracking_start = end;
            Cluster {
                flags,
                text_range: start..end,
                is_replaced,
            }
        })
    }

    pub fn clusters(&self) -> impl Iterator<Item = Cluster> + '_ {
        self.clusters_for_range(&self.full_range())
    }

    fn full_range(&self) -> ClusterRange {
        let end = self
            .cluster_ends
            .last()
            .map(|pos| (*pos as usize) >> 2)
            .unwrap_or(0);
        ClusterRange {
            text: 0..end,
            clusters: 0..self.cluster_flags.len(),
        }
    }

    pub(super) fn num_clusters(&self) -> usize {
        self.cluster_flags.len()
    }
}

impl TextAnalysis {
    pub(super) fn next_object(&mut self) -> ObjectHandle {
        let handle = self.num_objects;
        self.num_objects += 1;
        handle
    }

    pub(super) fn push_element(&mut self, element: Element) {
        self.elements.push(element);
    }

    pub(super) fn push_cluster(&mut self, cluster: &PendingCluster, is_replaced: bool) {
        println!(
            "pushing cluster with char {:?}, text {:?}, replaced: {is_replaced:}",
            cluster.base_char,
            cluster.range.clone()
        );
        if cluster.range.is_empty() {
            return;
        }
        let cluster_start = self.cluster_flags.len();
        self.cluster_flags.push(cluster.info);
        self.cluster_ends
            .push((cluster.range.end as u32) << 2 | is_replaced as u32);
        if !is_replaced {
            let mut next = cluster.script;
            if cluster.info.is_emoji() {
                next = match cluster.info.content() {
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
                    last_script_segment.range.clusters.end = self.cluster_flags.len();
                    last_script_segment.range.text.end = cluster.range.end;
                } else {
                    let cluster_end = self.cluster_flags.len();
                    self.script_segments.push(ScriptBidiSegment {
                        script,
                        range: ClusterRange {
                            text: cluster.range.clone(),
                            clusters: cluster_start..cluster_end,
                        },
                    })
                }
            } else {
                let cluster_end = self.cluster_flags.len();
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
}
