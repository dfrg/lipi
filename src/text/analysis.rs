//! Type to hold text analysis results.

use super::{is_real_script, BidiLevel, Cluster, ClusterAttributes, Language, PendingCluster};
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
}

/// A text segment.
#[derive(Clone, Debug)]
pub struct TextSegment {
    /// The Unicode script.
    pub script: Script,
    /// The user specified language.
    pub language: Option<Language>,
    /// The resolved bidirectional level.
    pub bidi_level: BidiLevel,
    /// The cluster range for the text.
    pub clusters: Range<usize>,
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
    pub level: BidiLevel,
    /// The range of segments covered.
    pub segments: Range<usize>,
}

/// Results of segmentation and classification of clusters.
#[derive(Clone, Default)]
pub struct ClusterAnalysis {
    pub(super) flags: Vec<ClusterAttributes>,
    pub(super) ends: Vec<u32>,
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
        let start = self.text_start(index)?;
        let end_with_flags = *self.ends.get(index)?;
        let end = (end_with_flags >> 2) as usize;
        Some(Cluster {
            attributes: flags,
            text_range: start..end,
        })
    }

    /// Returns an iterator over the sequence of clusters.
    pub fn iter(&self) -> impl Iterator<Item = Cluster> + '_ {
        self.iter_range(0..self.flags.len())
    }

    /// Returns an iterator over the sequence of clusters in the given range.
    pub fn iter_range(&self, range: Range<usize>) -> impl Iterator<Item = Cluster> + '_ {
        let mut tracking_start = self.text_range(range.clone()).map(|r| r.start).unwrap_or(0);
        let flags = self
            .flags
            .get(range.clone())
            .unwrap_or_default()
            .iter()
            .copied();
        let ends = self
            .ends
            .get(range.clone())
            .unwrap_or_default()
            .iter()
            .copied();
        flags.zip(ends).map(move |(flags, end)| {
            let start = tracking_start;
            let end = (end >> 2) as usize;
            tracking_start = end;
            Cluster {
                attributes: flags,
                text_range: start..end,
            }
        })
    }

    /// Returns the text range for the given range of clusters.
    pub fn text_range(&self, clusters: Range<usize>) -> Option<Range<usize>> {
        let text_start = self.text_start(clusters.start)?;
        if clusters.is_empty() {
            Some(text_start..text_start)
        } else {
            let text_end = (*self.ends.get(clusters.end.saturating_sub(1))? as usize) >> 2;
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
            (*self.ends.get(index - 1)? as usize) >> 2
        };
        Some(start)
    }
}

impl ClusterAnalysis {
    pub(super) fn push(&mut self, cluster: &PendingCluster) {
        self.flags.push(cluster.attrs);
        self.ends.push((cluster.range.end as u32) << 2);
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
