/*!
Font independent text analysis support for shaping and layout.
*/

// #![no_std]

extern crate alloc;

pub mod text;

mod element;

pub use element::{Element, ElementHandle, ElementKind, ObjectHandle};
pub use parlance::{Language, Script};

/// Maximum length of text supported by analysis passes.
///
/// Currently set to 2^30 or about 1 GB. Anything approaching this size likely
/// requires more sophisticated data storage and management anyway.
pub const MAX_TEXT_LEN: usize = 1 << 30;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_len_free_bits() {
        assert_eq!(MAX_TEXT_LEN & 0b1100_0000_0000_0000, 0);
    }
}
