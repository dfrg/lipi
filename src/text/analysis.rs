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
}

/// Results of text analysis.
#[derive(Clone, Default)]
pub struct TextAnalysis {
    pub(super) clusters: Vec<(ClusterFlags, u8)>,
    elements: Vec<Element>,
    pub(super) script_segments: Vec<ScriptBidiSegment>,
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

    pub fn clusters_for_range(
        &self,
        range: &ClusterRange,
    ) -> impl Iterator<Item = (ClusterFlags, Range<usize>, bool)> + '_ {
        let mut tracking_start = range.text.start;
        let mut clusters = self
            .clusters
            .get(range.clusters.clone())
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

    pub(super) fn num_clusters(&self) -> usize {
        self.clusters.len()
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

    pub(crate) fn cluster_ranges2(
        &self,
    ) -> impl Iterator<Item = (ClusterFlags, Range<usize>, bool, usize)> + '_ {
        let mut tracking_start = 0;
        let mut clusters = self.clusters.iter().copied().enumerate();
        core::iter::from_fn(move || {
            let (mut idx, (info, len)) = clusters.next()?;
            let start = tracking_start;
            let mut end = start + (len >> 2) as usize;
            let is_replacement = len & Self::CLUSTER_REPLACEMENT != 0;
            if len & Self::CLUSTER_CONTINUES != 0 {
                while let Some((cont_idx, (_, len))) = clusters.next() {
                    end += (len >> 2) as usize;
                    if len & Self::CLUSTER_CONTINUES == 0 {
                        break;
                    }
                    idx = cont_idx;
                }
            }
            tracking_start = end;
            Some((info, start..end, is_replacement, idx + 1))
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

    pub(super) fn push_cluster(&mut self, cluster: &PendingCluster, is_replacement: bool) {
        println!(
            "pushing cluster with char {:?}, text {:?}, replacement: {is_replacement:}",
            cluster.base_char,
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
