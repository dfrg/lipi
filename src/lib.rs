/*!
Font independent text analysis support for shaping and layout.
*/

// #![no_std]

extern crate alloc;

pub mod text;

mod element;
mod range;

pub use element::{Element, ElementHandle, ElementKind, ObjectHandle};
pub use parlance::{Language, Script};

/// Maximum inclusive text length supported by analysis passes.
///
/// Set to 2^30 - 1, leaving two low bits free when packing text end offsets
/// and flags into a u32 in analysis internals.
pub const MAX_TEXT_LEN: usize = (1 << 30) - 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_len_free_bits() {
        assert!(MAX_TEXT_LEN <= ((u32::MAX >> 2) as usize));
    }
}
