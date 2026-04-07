//! Source elements for input to text analysis.

use super::{BidiDirection, BidiOverride};
use crate::ElementHandle;

/// Determines how a marker reacts to bidirectional analysis and line breaking.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MarkerAffinity {
    /// Binds to the previous element.
    Previous,
    /// Binds to the next element.
    Next,
    /// Free to move independent of surrounding content.
    Independent(BidiDirection),
}

impl Default for MarkerAffinity {
    fn default() -> Self {
        Self::Independent(BidiDirection::Auto)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Default, Debug)]
/// Arbitrary object in a layout.
pub struct Marker {
    /// How the marker binds to surrounding content.
    pub affinity: MarkerAffinity,
    /// Length of text that the marker replaces. May be 0.
    pub len: u32,
    /// True if the marker causes a break in segmentation and shaping.
    pub breaks_segmentation: bool,
}

pub enum BidiControl {}

/// The type of a source element.
#[derive(Copy, Clone, Debug)]
pub enum SourceElementKind {
    /// A sequence of characters of the given length.
    Text(u32),
    /// An inline object with direction and length.
    ///
    /// If the length is greater than 0 then the object *replaces* that
    /// range of the source text.
    Object(BidiDirection, u32),
    /// The start of a region with some arbitrary identifier.
    StartSpan(u64),
    /// The end of a region with some arbitrary identifier.
    EndSpan(u64),
    /// Marks a position with some arbitrary identifier.
    Marker(u64),
    /// Start bidirectional override.
    PushBidiOverride(BidiOverride),
    /// End bidirectional override.
    PopBidiOverride,
    /// Start bidirectional isolate.
    PushBidiIsolate(BidiDirection),
    /// End bidirectional isolate.
    PopBidiIsolate,
    /// Prevents segmentation and shaping across neighboring elements.
    ///
    /// The typical use is to avoid shaping across visual boundaries such as
    /// the start or end of a span that has non-zero borders, margin or
    /// padding.
    BreakSegmentation,
}

impl Default for SourceElementKind {
    fn default() -> Self {
        Self::Text(0)
    }
}

/// A source element.
#[derive(Copy, Clone, Default, Debug)]
pub struct SourceElement {
    /// Client defined handle used for property retrieval.
    pub handle: ElementHandle,
    /// The type of the element.
    pub kind: SourceElementKind,
}
