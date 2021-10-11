/*!
Font independent text analysis support for shaping and layout.
*/

#![no_std]

// Avoid errors for generated Unicode data.

mod compose;

#[allow(clippy::upper_case_acronyms)]
mod unicode_data;

pub mod cluster;
pub mod locale;
pub mod paragraph;
pub mod unicode;
