use core::ops::Range;

/// A handle for an element.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct ElementHandle {
    /// Some identifier associated with the element.
    pub id: u64,
    /// Some identifier that describes the context for the element.
    pub context_id: u64,
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
    /// An inline object with a handle.
    Object(ObjectHandle),
    /// The start of a region.
    StartSpan,
    /// The end of a region.
    EndSpan,
    /// Marks an arbitrary position.
    Marker,
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
}

impl Element {
    /// Returns the text range for the element.
    pub fn text_range(&self) -> Range<usize> {
        let start = self.text_start as usize;
        let len = match &self.kind {
            ElementKind::Text(len) => *len,
            _ => 0,
        };
        start..start + len as usize
    }
}
