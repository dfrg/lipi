/*!
Font independent text analysis support for shaping and layout.
*/

// #![no_std]

extern crate alloc;

pub mod text;

mod element;
mod properties;

pub use element::{Element, ElementHandle, ElementKind, SourceElement, SourceElementKind};
pub use parlance::{Language, Script, WordBreak};
pub use properties::LineBreak;
