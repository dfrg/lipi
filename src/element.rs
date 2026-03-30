use std::ops::Range;

use parlance::{BidiDirection, BidiOverride};

/// A handle for an element.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct ElementHandle {
    /// Some identifier associated with the element.
    pub id: u64,
    /// Some identifier that describes the context for the element.
    pub context_id: u64,
}

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

/// Handle for an inline object.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct ObjectHandle(pub u32);

/// The type of a processed element.
#[derive(Copy, Clone, Debug)]
pub enum ElementKind {
    /// A sequence of characters of the given length.
    Text(u32),
    /// An inline object with a handle and length.
    ///
    /// If the length is greater than 0 then the object *replaces* that
    /// range of the source text.
    Object(ObjectHandle, u32),
    /// The start of a region with some arbitrary identifier.
    StartSpan(u64),
    /// The end of a region with some arbitrary identifier.
    EndSpan(u64),
    /// Marks a position with some arbitrary identifier.
    Marker(u64),
}

/// A processed element.
#[derive(Copy, Clone, Debug)]
pub struct Element {
    /// Client defined handle used for property retrieval.
    pub handle: ElementHandle,
    /// The type of the element.
    pub kind: ElementKind,
    /// The beginning of this element in the source text.
    pub(crate) text_start: u32,
    /// True if we should break shaping after the previous element.
    pub(crate) break_shaping_before: bool,
}

impl Element {
    /// Returns the text range for the element.
    pub fn text_range(&self) -> Range<usize> {
        let start = self.text_start as usize;
        let len = match &self.kind {
            ElementKind::Text(len) | ElementKind::Object(_, len) => *len,
            _ => 0,
        };
        start..start + len as usize
    }

    /// Returns true if the element should break a shaping run.
    pub fn break_shaping_before(&self) -> bool {
        self.break_shaping_before
    }
}
