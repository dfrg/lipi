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
    Text(usize),
    /// An inline object with direction and length.
    ///
    /// If the length is greater than 0 then the object *replaces* that
    /// range of the source text.
    Object(BidiDirection, usize),
    /// The start of a span.
    StartSpan,
    /// The end of a span.
    EndSpan,
    /// Start bidirectional override.
    StartBidiOverride(BidiOverride),
    /// End bidirectional override.
    EndBidiOverride,
    /// Start bidirectional isolate.
    PushBidiIsolate(BidiDirection),
    /// End bidirectional isolate.
    PopBidiIsolate,
    /// An element that prevents shaping across the neighboring elements.
    ///
    /// The typical use is to avoid shaping across visual boundaries such as
    /// the start or end of a span that has non-zero borders, margin or
    /// padding.
    BreakShaping,
    /// An arbitrary marker element with some identifier.
    Marker(u64),
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
pub type ObjectHandle = usize;

/// The type of a processed element.
#[derive(Copy, Clone, Debug)]
pub enum ElementKind {
    /// A sequence of characters of the given length.
    Text(usize),
    /// An inline object with a handle and length.
    ///
    /// If the length is greater than 0 then the object *replaces* that
    /// range of the source text.
    Object(ObjectHandle, usize),
    /// The start of a span.
    StartSpan,
    /// The end of a span.
    EndSpan,
    /// An arbitrary marker element with some identifier.
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
    pub(crate) text_start: usize,
    /// True if we should break shaping after the previous element.
    pub(crate) break_shaping_before: bool,
}
