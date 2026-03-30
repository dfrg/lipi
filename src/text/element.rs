//! Source elements for input to text analysis.

use super::{BidiDirection, BidiOverride};
use crate::ElementHandle;

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
