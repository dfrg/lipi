/*!
Script aware cluster segmentation.

This module provides support for breaking text into clusters that are
appropriate for shaping with a given script. For most scripts, clusters are
equivalent to Unicode grapheme clusters. More complex scripts, however,
may produce shaping clusters that contain multiple graphemes.
*/

mod char;
#[allow(clippy::module_inception)]
mod cluster;
mod complex;
mod info;
mod myanmar;
mod parse;
mod simple;

pub use self::{
    char::{Char, ShapeClass, SourceChar},
    cluster::{Cluster, Status, MAX_CLUSTER_SIZE},
    info::{CharInfo, ClusterInfo, Emoji, Whitespace},
    parse::Parser,
};

use super::unicode::*;
use super::unicode_data;

/// Artibrary user data that can be associated with a character throughout
/// the shaping pipeline.
pub type UserData = u32;
