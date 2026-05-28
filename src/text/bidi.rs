// Copyright 2021 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Unicode bidirectional algorithm.

use alloc::vec::Vec;

/// Type for a bidirectional level.
pub type BidiLevel = u8;

/// Bidirectional class value using ICU's numeric `UCharDirection` representation.
///
/// The numeric values used here match ICU4C's bidi class constants so custom
/// Unicode engines can pass through ICU-derived data without remapping.
/// See <https://unicode-org.github.io/icu-docs/apidoc/dev/icu4c/ubidi_8h.html>
/// and the `UCharDirection` enum definition for the source values.
#[derive(Copy, Clone, Default, Eq, PartialEq, Debug)]
pub struct BidiClass(pub(crate) u8);

impl BidiClass {
    /// Creates a bidi class from its ICU numeric value.
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    /// Left-to-right letter (L).
    pub const LEFT_TO_RIGHT: Self = Self(0);
    /// Right-to-left letter (R).
    pub const RIGHT_TO_LEFT: Self = Self(1);
    /// European number (EN).
    pub const EUROPEAN_NUMBER: Self = Self(2);
    /// European separator (ES).
    pub const EUROPEAN_SEPARATOR: Self = Self(3);
    /// European terminator (ET).
    pub const EUROPEAN_TERMINATOR: Self = Self(4);
    /// Arabic number (AN).
    pub const ARABIC_NUMBER: Self = Self(5);
    /// Common separator (CS).
    pub const COMMON_SEPARATOR: Self = Self(6);
    /// Paragraph separator (B).
    pub const PARAGRAPH_SEPARATOR: Self = Self(7);
    /// Segment separator (S).
    pub const SEGMENT_SEPARATOR: Self = Self(8);
    /// Whitespace (WS).
    pub const WHITE_SPACE: Self = Self(9);
    /// Other neutral (ON).
    pub const OTHER_NEUTRAL: Self = Self(10);
    /// Left-to-right embedding (LRE).
    pub const LEFT_TO_RIGHT_EMBEDDING: Self = Self(11);
    /// Left-to-right override (LRO).
    pub const LEFT_TO_RIGHT_OVERRIDE: Self = Self(12);
    /// Arabic letter (AL).
    pub const ARABIC_LETTER: Self = Self(13);
    /// Right-to-left embedding (RLE).
    pub const RIGHT_TO_LEFT_EMBEDDING: Self = Self(14);
    /// Right-to-left override (RLO).
    pub const RIGHT_TO_LEFT_OVERRIDE: Self = Self(15);
    /// Pop directional format (PDF).
    pub const POP_DIRECTIONAL_FORMAT: Self = Self(16);
    /// Nonspacing mark (NSM).
    pub const NONSPACING_MARK: Self = Self(17);
    /// Boundary neutral (BN).
    pub const BOUNDARY_NEUTRAL: Self = Self(18);
    /// First strong isolate (FSI).
    pub const FIRST_STRONG_ISOLATE: Self = Self(19);
    /// Left-to-right isolate (LRI).
    pub const LEFT_TO_RIGHT_ISOLATE: Self = Self(20);
    /// Right-to-left isolate (RLI).
    pub const RIGHT_TO_LEFT_ISOLATE: Self = Self(21);
    /// Pop directional isolate (PDI).
    pub const POP_DIRECTIONAL_ISOLATE: Self = Self(22);

    const fn mask(self) -> u32 {
        1 << (self.0 as u32)
    }

    pub(crate) const fn from_icu4c_value(value: u8) -> Self {
        Self::new(value)
    }
}

/// Paired bracket metadata used by the bidi algorithm.
///
/// The contained character is the paired bracket returned by the Unicode data
/// source, and the variant identifies whether the source character is an open
/// or close bracket for bidi pair resolution.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum BidiBracket {
    /// The source character is an opening bracket paired with the given char.
    Open(char),
    /// The source character is a closing bracket paired with the given char.
    Close(char),
}

type BracketEntry = (usize, char, BidiBracket);

/// Resolver for the Unicode bidirectional algorithm.
#[derive(Clone, Default)]
pub(crate) struct BidiResolver {
    base_level: BidiLevel,
    levels: Vec<BidiLevel>,
    types: Vec<BidiClass>,
    bracket_pairs: Vec<(usize, usize)>,
    runs: Vec<Run>,
    indices: Vec<usize>,
    flags: u16,
}

impl BidiResolver {
    /// Returns the base level of the text.
    pub(crate) fn base_level(&self) -> u8 {
        self.base_level
    }

    /// Returns the sequence of bidi levels corresponding to all characters in the
    /// paragraph.
    pub(crate) fn levels(&self) -> &[BidiLevel] {
        &self.levels
    }

    /// Clears the resolver state.
    pub(crate) fn clear(&mut self) {
        self.levels.clear();
        self.types.clear();
        self.bracket_pairs.clear();
        self.flags = 0;
        self.base_level = 0;
    }

    /// Resolves a paragraph with the specified base direction and
    /// precomputed types.
    pub(crate) fn resolve(
        &mut self,
        initial_types: &[BidiClass],
        brackets: &[BracketEntry],
        base_level: Option<u8>,
    ) {
        self.clear();
        self.base_level = match base_level {
            Some(level) => level & 1,
            _ => Self::default_level(initial_types),
        };
        let len = initial_types.len();
        // if !needs_bidi && self.base_level == 0 {
        //     self.flags |= 1;
        //     self.levels.resize(initial_types.len(), self.base_level);
        //     return;
        // }
        self.types.extend_from_slice(initial_types);
        self.resolve_levels();
        self.resolve_runs(initial_types);
        //self.dump_sequences();
        for i in 0..self.runs.len() {
            if self.runs[i].in_sequence {
                continue;
            }
            self.types.truncate(len);
            self.indices.clear();
            let mut cur = i;
            let level = self.runs[i].level;
            let sos = self.runs[i].sos;
            let mut eos;
            loop {
                let run = &self.runs[cur];
                for i in run.start..run.end {
                    let ty = self.types[i];
                    if !is_removed_by_x9(ty) {
                        self.types.push(ty);
                        self.indices.push(i);
                    }
                }
                eos = run.eos;
                cur = match run.next {
                    Some(i) => i,
                    None => break,
                };
            }
            self.resolve_sequence(initial_types, brackets, level, sos, eos, self.indices.len());
        }
        for i in 0..len {
            let t = initial_types[i];
            if t == BidiClass::SEGMENT_SEPARATOR || t == BidiClass::PARAGRAPH_SEPARATOR {
                self.levels[i] = self.base_level;
                for j in (0..i).rev() {
                    let t = initial_types[j];
                    if is_removed_by_x9(t) {
                        continue;
                    } else if t == BidiClass::WHITE_SPACE
                        || is_isolate_initiator(t)
                        || t == BidiClass::POP_DIRECTIONAL_ISOLATE
                    {
                        self.levels[j] = self.base_level;
                    } else {
                        break;
                    }
                }
            } else if is_removed_by_x9(t) {
                if i == 0 {
                    self.levels[i] = self.base_level;
                } else {
                    self.levels[i] = self.levels[i - 1];
                }
                //self.levels[i] = 0xFF;
            }
        }
    }

    /// Optional post-processing step that resets trailing neutrals and isolate
    /// formatting characters to the paragraph base level.
    ///
    /// This should generally be applied after line breaking and is therefore
    /// not part of the default paragraph-level `resolve` flow.
    pub(crate) fn resolve_trailing_neutrals(&mut self, initial_types: &[BidiClass]) {
        let len = initial_types.len();
        for i in (0..len).rev() {
            let t = initial_types[i];
            if is_removed_by_x9(t) {
                continue;
            }
            if t == BidiClass::WHITE_SPACE
                || is_isolate_initiator(t)
                || t == BidiClass::POP_DIRECTIONAL_ISOLATE
            {
                self.levels[i] = self.base_level;
            } else {
                break;
            }
        }
    }

    fn default_level(types: &[BidiClass]) -> u8 {
        let mut isolates = 0;
        for ty in types {
            let ty = *ty;
            match ty {
                BidiClass::RIGHT_TO_LEFT_ISOLATE
                | BidiClass::LEFT_TO_RIGHT_ISOLATE
                | BidiClass::FIRST_STRONG_ISOLATE => isolates += 1,
                BidiClass::POP_DIRECTIONAL_ISOLATE => {
                    if isolates > 0 {
                        isolates -= 1;
                    }
                }
                BidiClass::LEFT_TO_RIGHT | BidiClass::RIGHT_TO_LEFT | BidiClass::ARABIC_LETTER => {
                    if isolates == 0 {
                        return if ty == BidiClass::LEFT_TO_RIGHT { 0 } else { 1 };
                    }
                }
                _ => {}
            }
        }
        0
    }

    fn default_level_until_pdi(types: &[BidiClass]) -> u8 {
        let mut isolates = 0;
        for ty in types {
            let ty = *ty;
            match ty {
                BidiClass::RIGHT_TO_LEFT_ISOLATE
                | BidiClass::LEFT_TO_RIGHT_ISOLATE
                | BidiClass::FIRST_STRONG_ISOLATE => isolates += 1,
                BidiClass::POP_DIRECTIONAL_ISOLATE => {
                    if isolates > 0 {
                        isolates -= 1;
                    } else {
                        return 0;
                    }
                }
                BidiClass::LEFT_TO_RIGHT | BidiClass::RIGHT_TO_LEFT | BidiClass::ARABIC_LETTER => {
                    if isolates == 0 {
                        return if ty == BidiClass::LEFT_TO_RIGHT { 0 } else { 1 };
                    }
                }
                _ => {}
            }
        }
        0
    }

    fn resolve_levels(&mut self) {
        let base = self.base_level;
        let len = self.types.len();
        self.levels.clear();
        self.levels.resize(len, 0);
        let mut stack = Stack::new();
        let mut overflow_isolates = 0;
        let mut overflow_embedding = 0;
        let mut valid_isolates = 0;
        stack.push(base, BidiClass::OTHER_NEUTRAL, false);
        for i in 0..len {
            let t = self.types[i];
            let tmask = t.mask();
            if tmask & EXPLICIT_MASK != 0 {
                let is_isolate = tmask & ISOLATE_MASK != 0;
                let is_rtl = if t == BidiClass::FIRST_STRONG_ISOLATE && i + 1 < len {
                    Self::default_level_until_pdi(&self.types[i + 1..]) == 1
                } else {
                    tmask & RTL_MASK != 0
                };
                if is_isolate {
                    self.levels[i] = stack.embedding_level();
                    let os = stack.override_status();
                    if os != BidiClass::OTHER_NEUTRAL {
                        self.types[i] = os;
                    }
                }
                let new_level = if is_rtl {
                    (stack.embedding_level() + 1) | 1
                } else {
                    (stack.embedding_level() + 2) & !1
                };
                if new_level <= MAX_STACK as u8 && overflow_isolates == 0 && overflow_embedding == 0
                {
                    if is_isolate {
                        valid_isolates += 1;
                    }
                    stack.push(
                        new_level,
                        if t == BidiClass::LEFT_TO_RIGHT_OVERRIDE {
                            BidiClass::LEFT_TO_RIGHT
                        } else if t == BidiClass::RIGHT_TO_LEFT_OVERRIDE {
                            BidiClass::RIGHT_TO_LEFT
                        } else {
                            BidiClass::OTHER_NEUTRAL
                        },
                        is_isolate,
                    );
                } else if is_isolate {
                    overflow_isolates += 1;
                } else if overflow_isolates == 0 {
                    overflow_embedding += 1;
                }
            } else if t == BidiClass::POP_DIRECTIONAL_ISOLATE {
                if overflow_isolates > 0 {
                    overflow_isolates -= 1;
                } else if valid_isolates == 0 {
                    // empty
                } else {
                    overflow_embedding = 0;
                    while !stack.isolate_status() {
                        stack.pop();
                    }
                    stack.pop();
                    valid_isolates -= 1;
                }
                self.levels[i] = stack.embedding_level();
                if stack.override_status() != BidiClass::OTHER_NEUTRAL {
                    self.types[i] = stack.override_status();
                }
            } else if t == BidiClass::POP_DIRECTIONAL_FORMAT {
                self.levels[i] = stack.embedding_level();
                if overflow_isolates > 0 {
                    // empty
                } else if overflow_embedding > 0 {
                    overflow_embedding -= 1;
                } else if !stack.isolate_status() && stack.depth >= 2 {
                    stack.pop();
                }
            } else if t == BidiClass::PARAGRAPH_SEPARATOR {
                stack.depth = 1;
                overflow_isolates = 0;
                overflow_embedding = 0;
                valid_isolates = 0;
                self.levels[i] = base;
            } else if t != BidiClass::BOUNDARY_NEUTRAL {
                self.levels[i] = stack.embedding_level();
                if stack.override_status() != BidiClass::OTHER_NEUTRAL {
                    self.types[i] = stack.override_status();
                }
            }
        }
    }

    fn resolve_runs(&mut self, initial_types: &[BidiClass]) {
        let len = self.types.len();
        self.runs.clear();
        let mut start = 0;
        while start < len {
            if !is_removed_by_x9(self.types[start]) {
                break;
            }
            start += 1;
        }
        if start == len {
            return;
        }
        let mut level = self.levels[start];
        let mut offset = 0;
        for i in start + 1..len {
            if is_removed_by_x9(self.types[i]) {
                continue;
            }
            if self.levels[i] != level {
                self.runs.push(Run::new(level, offset, i));
                offset = i;
                level = self.levels[i];
            }
        }
        if offset < len {
            self.runs.push(Run::new(level, offset, len));
        }
        for run in &mut self.runs {
            while run.start < run.end {
                if is_removed_by_x9(self.types[run.start]) {
                    run.start += 1;
                } else {
                    break;
                }
            }
            while run.end > run.start {
                if is_removed_by_x9(self.types[run.end - 1]) {
                    run.end -= 1;
                } else {
                    break;
                }
            }
            if run.start == run.end {
                continue;
            }
            if self.types[run.start] == BidiClass::POP_DIRECTIONAL_ISOLATE {
                run.starts_with_pdi = true;
            }
            let mut prev_level = self.base_level;
            for i in (0..run.start).rev() {
                if !is_removed_by_x9(self.types[i]) {
                    prev_level = self.levels[i];
                    break;
                }
            }
            run.sos = type_from_level(prev_level.max(run.level));
            if is_isolate_initiator(initial_types[run.end - 1]) {
                run.ends_with_isolate = true;
                run.eos = type_from_level(self.base_level.max(run.level));
            } else {
                let mut next_level = self.base_level;
                for i in run.end..len {
                    if !is_removed_by_x9(self.types[i]) {
                        next_level = self.levels[i];
                        break;
                    }
                }
                run.eos = type_from_level(next_level.max(run.level));
            }
        }
        for i in 0..self.runs.len() {
            if self.runs[i].ends_with_isolate {
                let level = self.runs[i].level;
                for j in i + 1..self.runs.len() {
                    if self.runs[j].starts_with_pdi && self.runs[j].level == level {
                        self.runs[i].next = Some(j);
                        self.runs[j].in_sequence = true;
                        break;
                    }
                }
            }
        }
    }

    #[allow(clippy::needless_range_loop)]
    fn resolve_sequence(
        &mut self,
        initial_types: &[BidiClass],
        brackets: &[BracketEntry],
        level: u8,
        sos: BidiClass,
        eos: BidiClass,
        len: usize,
    ) {
        if len == 0 {
            return;
        }
        const W1_MASK: u32 = BidiClass::LEFT_TO_RIGHT_ISOLATE.mask()
            | BidiClass::RIGHT_TO_LEFT_ISOLATE.mask()
            | BidiClass::FIRST_STRONG_ISOLATE.mask()
            | BidiClass::POP_DIRECTIONAL_ISOLATE.mask();
        const W2_MASK: u32 = BidiClass::LEFT_TO_RIGHT.mask()
            | BidiClass::RIGHT_TO_LEFT.mask()
            | BidiClass::ARABIC_LETTER.mask();
        const W4_MASK: u32 =
            BidiClass::EUROPEAN_SEPARATOR.mask() | BidiClass::COMMON_SEPARATOR.mask();
        let mut prev = sos;
        let mut prev_strong = prev;
        let types = &mut self.types[initial_types.len()..];
        for i in 0..len {
            let mut t = types[i];
            let tmask = t.mask();
            if t == BidiClass::NONSPACING_MARK {
                // W1
                types[i] = prev;
            } else {
                if tmask & W1_MASK != 0 {
                    prev = BidiClass::OTHER_NEUTRAL;
                    continue;
                }
                if t == BidiClass::EUROPEAN_NUMBER {
                    // W2
                    if prev_strong == BidiClass::ARABIC_LETTER {
                        t = BidiClass::ARABIC_NUMBER;
                        types[i] = t;
                    }
                } else if tmask & W2_MASK != 0 {
                    prev_strong = t;
                    // W3
                    if t == BidiClass::ARABIC_LETTER {
                        t = BidiClass::RIGHT_TO_LEFT;
                        types[i] = t;
                    }
                } else if tmask & W4_MASK != 0 && i < (len - 1) {
                    // W4
                    let mut next = types[i + 1];
                    if next == BidiClass::EUROPEAN_NUMBER && prev_strong == BidiClass::ARABIC_LETTER
                    {
                        next = BidiClass::ARABIC_NUMBER;
                    }
                    if prev == BidiClass::EUROPEAN_NUMBER && next == BidiClass::EUROPEAN_NUMBER {
                        t = BidiClass::EUROPEAN_NUMBER;
                        types[i] = t;
                    } else if t == BidiClass::COMMON_SEPARATOR
                        && prev == BidiClass::ARABIC_NUMBER
                        && next == BidiClass::ARABIC_NUMBER
                    {
                        t = BidiClass::ARABIC_NUMBER;
                        types[i] = t;
                    }
                }
                prev = t;
            }
        }
        // W5
        let mut i = 0;
        while i < len {
            if types[i] == BidiClass::EUROPEAN_TERMINATOR {
                let limit = find_limit(types, i, BidiClass::EUROPEAN_TERMINATOR);
                let mut t = if i == 0 { sos } else { types[i - 1] };
                if t != BidiClass::EUROPEAN_NUMBER {
                    t = if limit == len { eos } else { types[limit] };
                }
                if t == BidiClass::EUROPEAN_NUMBER {
                    for j in i..limit {
                        types[j] = BidiClass::EUROPEAN_NUMBER;
                    }
                }
                i = limit;
            }
            i += 1;
        }
        // W6, W7
        const W6_MASK: u32 = BidiClass::EUROPEAN_SEPARATOR.mask()
            | BidiClass::EUROPEAN_TERMINATOR.mask()
            | BidiClass::COMMON_SEPARATOR.mask();
        prev_strong = sos;
        for i in 0..len {
            let t = types[i];
            if t.mask() & W6_MASK != 0 {
                // W6
                types[i] = BidiClass::OTHER_NEUTRAL;
            } else if t == BidiClass::EUROPEAN_NUMBER {
                // W7
                if prev_strong == BidiClass::LEFT_TO_RIGHT {
                    types[i] = BidiClass::LEFT_TO_RIGHT;
                }
            } else if t == BidiClass::LEFT_TO_RIGHT || t == BidiClass::RIGHT_TO_LEFT {
                prev_strong = t;
            }
        }
        // N0
        if !brackets.is_empty() {
            let base_brackets = self.bracket_pairs.len();
            let mut bracket_stack = BracketStack::new();
            for i in 0..len {
                if types[i] != BidiClass::OTHER_NEUTRAL {
                    continue;
                }
                let index = self.indices[i];
                if let Ok(index) = brackets.binary_search_by(|x| x.0.cmp(&index)) {
                    let (_, ch, bracket) = brackets[index];
                    match bracket {
                        BidiBracket::Open(closer) => {
                            if bracket_stack.depth == MAX_BRACKET_STACK {
                                break;
                            }
                            bracket_stack.push(i, closer);
                        }
                        BidiBracket::Close(_) => {
                            if let Some(open) = bracket_stack.find_and_pop(ch) {
                                self.bracket_pairs.push((open, i));
                            }
                        }
                    }
                }
            }
            if self.bracket_pairs.len() > base_brackets {
                let embed_dir = if level & 1 != 0 {
                    BidiClass::RIGHT_TO_LEFT
                } else {
                    BidiClass::LEFT_TO_RIGHT
                };
                let bracket_pairs = &mut self.bracket_pairs[base_brackets..];
                bracket_pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                for pair in bracket_pairs {
                    let mut pair_dir = BidiClass::OTHER_NEUTRAL;
                    for i in pair.0 + 1..pair.1 {
                        let dir = match types[i] {
                            BidiClass::EUROPEAN_NUMBER
                            | BidiClass::ARABIC_NUMBER
                            | BidiClass::ARABIC_LETTER
                            | BidiClass::RIGHT_TO_LEFT => BidiClass::RIGHT_TO_LEFT,
                            BidiClass::LEFT_TO_RIGHT => BidiClass::LEFT_TO_RIGHT,
                            _ => BidiClass::OTHER_NEUTRAL,
                        };
                        if dir == BidiClass::OTHER_NEUTRAL {
                            continue;
                        }
                        pair_dir = dir;
                        if dir == embed_dir {
                            break;
                        }
                    }
                    if pair_dir == BidiClass::OTHER_NEUTRAL {
                        pair.0 = self.indices[pair.0];
                        pair.1 = self.indices[pair.1];
                        continue;
                    }
                    if pair_dir != embed_dir {
                        pair_dir = sos;
                        for i in (0..pair.0).rev() {
                            let dir = match types[i] {
                                BidiClass::EUROPEAN_NUMBER
                                | BidiClass::ARABIC_NUMBER
                                | BidiClass::ARABIC_LETTER
                                | BidiClass::RIGHT_TO_LEFT => BidiClass::RIGHT_TO_LEFT,
                                BidiClass::LEFT_TO_RIGHT => BidiClass::LEFT_TO_RIGHT,
                                _ => BidiClass::OTHER_NEUTRAL,
                            };
                            if dir != BidiClass::OTHER_NEUTRAL {
                                pair_dir = dir;
                                break;
                            }
                        }
                        if pair_dir == embed_dir || pair_dir == BidiClass::OTHER_NEUTRAL {
                            pair_dir = embed_dir;
                        }
                    }
                    types[pair.0] = pair_dir;
                    types[pair.1] = pair_dir;
                    for i in pair.0 + 1..pair.1 {
                        let index = self.indices[i];
                        if initial_types[index] == BidiClass::NONSPACING_MARK {
                            types[i] = pair_dir;
                        } else {
                            break;
                        }
                    }
                    for i in pair.1 + 1..len {
                        let index = self.indices[i];
                        if initial_types[index] == BidiClass::NONSPACING_MARK {
                            types[i] = pair_dir;
                        } else {
                            break;
                        }
                    }
                    pair.0 = self.indices[pair.0];
                    pair.1 = self.indices[pair.1];
                }
            }
        }
        // N1, N2
        const N_MASK: u32 = BidiClass::PARAGRAPH_SEPARATOR.mask()
            | BidiClass::SEGMENT_SEPARATOR.mask()
            | BidiClass::WHITE_SPACE.mask()
            | BidiClass::OTHER_NEUTRAL.mask()
            | BidiClass::RIGHT_TO_LEFT_ISOLATE.mask()
            | BidiClass::LEFT_TO_RIGHT_ISOLATE.mask()
            | BidiClass::FIRST_STRONG_ISOLATE.mask()
            | BidiClass::POP_DIRECTIONAL_ISOLATE.mask();
        let mut i = 0;
        while i < len {
            let t = types[i];
            if t.mask() & N_MASK != 0 {
                let offset = i;
                let limit = find_limit_by_mask(types, offset, N_MASK);
                let mut leading;
                let mut trailing;
                if offset == 0 {
                    leading = sos;
                } else {
                    leading = types[offset - 1];
                    if leading == BidiClass::ARABIC_NUMBER || leading == BidiClass::EUROPEAN_NUMBER
                    {
                        leading = BidiClass::RIGHT_TO_LEFT;
                    }
                }
                if limit == len {
                    trailing = eos;
                } else {
                    trailing = types[limit];
                    if trailing == BidiClass::ARABIC_NUMBER
                        || trailing == BidiClass::EUROPEAN_NUMBER
                    {
                        trailing = BidiClass::RIGHT_TO_LEFT;
                    }
                }
                let resolved = if leading == trailing {
                    // N1
                    leading
                } else {
                    // N2
                    if level & 1 != 0 {
                        BidiClass::RIGHT_TO_LEFT
                    } else {
                        BidiClass::LEFT_TO_RIGHT
                    }
                };
                for j in offset..limit {
                    types[j] = resolved;
                }
                i = limit - 1;
            }
            i += 1;
        }
        // Implicit levels
        if level & 1 == 0 {
            // I1
            for i in 0..len {
                let index = self.indices[i];
                let t = types[i];
                if t == BidiClass::RIGHT_TO_LEFT {
                    self.levels[index] = level + 1;
                } else if t != BidiClass::LEFT_TO_RIGHT {
                    self.levels[index] = level + 2;
                } else {
                    self.levels[index] = level;
                }
            }
        } else {
            // I2
            for i in 0..len {
                let index = self.indices[i];
                let t = types[i];
                if t != BidiClass::RIGHT_TO_LEFT {
                    self.levels[index] = level + 1;
                } else {
                    self.levels[index] = level;
                }
            }
        }
    }
}

/// Returns a default bidi type for a level.
pub(crate) fn type_from_level(level: BidiLevel) -> BidiClass {
    if level & 1 == 0 {
        BidiClass::LEFT_TO_RIGHT
    } else {
        BidiClass::RIGHT_TO_LEFT
    }
}

/// Computes an ordering for a sequence of bidi runs based on levels.
pub(crate) fn _reorder<F>(order: &mut [usize], levels: F)
where
    F: Fn(usize) -> BidiLevel,
{
    let mut max_level = 0;
    let mut lowest_odd_level = 255;
    for (i, o) in order.iter_mut().enumerate() {
        *o = i;
        let level = levels(i);
        if level > max_level {
            max_level = level;
        }
        if level & 1 != 0 && level < lowest_odd_level {
            lowest_odd_level = level;
        }
    }
    let len = order.len();
    for level in (lowest_odd_level..=max_level).rev() {
        let mut i = 0;
        while i < len {
            if levels(i) >= level {
                let mut end = i + 1;
                while end < len && levels(end) >= level {
                    end += 1;
                }
                let mut j = i;
                let mut k = end - 1;
                while j < k {
                    order.swap(j, k);
                    j += 1;
                    k -= 1;
                }
                i = end;
            }
            i += 1;
        }
    }
}

/// Returns whether the character needs bidirectional resolution.
#[inline(always)]
pub(crate) fn needs_bidi_resolution(bidi_class: BidiClass) -> bool {
    bidi_class.mask() & BIDI_MASK != 0
}

const OVERRIDE_MASK: u32 = BidiClass::RIGHT_TO_LEFT_EMBEDDING.mask()
    | BidiClass::LEFT_TO_RIGHT_EMBEDDING.mask()
    | BidiClass::RIGHT_TO_LEFT_OVERRIDE.mask()
    | BidiClass::LEFT_TO_RIGHT_OVERRIDE.mask();
const ISOLATE_MASK: u32 = BidiClass::RIGHT_TO_LEFT_ISOLATE.mask()
    | BidiClass::LEFT_TO_RIGHT_ISOLATE.mask()
    | BidiClass::FIRST_STRONG_ISOLATE.mask();
const EXPLICIT_MASK: u32 = OVERRIDE_MASK | ISOLATE_MASK;
const RTL_MASK: u32 = BidiClass::RIGHT_TO_LEFT_EMBEDDING.mask()
    | BidiClass::RIGHT_TO_LEFT_OVERRIDE.mask()
    | BidiClass::RIGHT_TO_LEFT_ISOLATE.mask();
const REMOVED_BY_X9_MASK: u32 =
    OVERRIDE_MASK | BidiClass::POP_DIRECTIONAL_FORMAT.mask() | BidiClass::BOUNDARY_NEUTRAL.mask();
const BIDI_MASK: u32 = EXPLICIT_MASK
    | BidiClass::RIGHT_TO_LEFT.mask()
    | BidiClass::ARABIC_LETTER.mask()
    | BidiClass::ARABIC_NUMBER.mask();
const _RESET_MASK: u32 =
    ISOLATE_MASK | BidiClass::POP_DIRECTIONAL_ISOLATE.mask() | BidiClass::WHITE_SPACE.mask();

fn is_isolate_initiator(ty: BidiClass) -> bool {
    ty.mask() & ISOLATE_MASK != 0
}

pub(crate) fn is_removed_by_x9(ty: BidiClass) -> bool {
    ty.mask() & REMOVED_BY_X9_MASK != 0
}

pub(crate) fn _is_reset(ty: BidiClass) -> bool {
    ty.mask() & _RESET_MASK != 0
}

fn find_limit(types: &[BidiClass], offset: usize, ty: BidiClass) -> usize {
    let mut len = offset;
    for &t in &types[offset..] {
        if t != ty {
            break;
        }
        len += 1;
    }
    len
}

fn find_limit_by_mask(types: &[BidiClass], offset: usize, mask: u32) -> usize {
    let mut len = offset;
    for &t in &types[offset..] {
        if t.mask() & mask == 0 {
            break;
        }
        len += 1;
    }
    len
}

#[derive(Clone)]
struct Run {
    level: u8,
    ends_with_isolate: bool,
    starts_with_pdi: bool,
    sos: BidiClass,
    eos: BidiClass,
    start: usize,
    end: usize,
    in_sequence: bool,
    next: Option<usize>,
}

impl Run {
    fn new(level: u8, start: usize, end: usize) -> Self {
        Self {
            level,
            ends_with_isolate: false,
            starts_with_pdi: false,
            sos: BidiClass::OTHER_NEUTRAL,
            eos: BidiClass::OTHER_NEUTRAL,
            start,
            end,
            in_sequence: false,
            next: None,
        }
    }
}

const MAX_STACK: usize = 125;

struct Stack {
    embedding_level: [u8; MAX_STACK + 1],
    override_status: [BidiClass; MAX_STACK + 1],
    isolate_status: [bool; MAX_STACK + 1],
    depth: usize,
}

impl Stack {
    fn new() -> Self {
        Self {
            depth: 0,
            embedding_level: [0; MAX_STACK + 1],
            override_status: [BidiClass::OTHER_NEUTRAL; MAX_STACK + 1],
            isolate_status: [false; MAX_STACK + 1],
        }
    }

    fn push(&mut self, level: u8, override_status: BidiClass, isolate_status: bool) {
        let d = self.depth;
        self.embedding_level[d] = level;
        self.override_status[d] = override_status;
        self.isolate_status[d] = isolate_status;
        self.depth += 1;
    }

    fn pop(&mut self) {
        if self.depth > 1 {
            self.depth -= 1;
        }
    }

    fn embedding_level(&self) -> u8 {
        self.embedding_level[self.depth - 1]
    }

    fn override_status(&self) -> BidiClass {
        self.override_status[self.depth - 1]
    }

    fn isolate_status(&self) -> bool {
        self.isolate_status[self.depth - 1]
    }
}

const MAX_BRACKET_STACK: usize = 63;

struct BracketStack {
    openers: [(usize, char); MAX_BRACKET_STACK],
    depth: usize,
}

impl BracketStack {
    fn new() -> Self {
        Self {
            openers: [(0, '\0'); MAX_BRACKET_STACK],
            depth: 0,
        }
    }

    fn push(&mut self, offset: usize, closer: char) {
        self.openers[self.depth] = (offset, closer);
        self.depth += 1;
    }

    fn find_and_pop(&mut self, closer: char) -> Option<usize> {
        if self.depth == 0 {
            return None;
        }
        for i in (0..self.depth).rev() {
            let c = self.openers[i].1;
            if c == closer
                || (c == '\u{232A}' && closer == '\u{3009}')
                || (c == '\u{3009}' && closer == '\u{232A}')
            {
                self.depth = i;
                return Some(self.openers[i].0);
            }
        }
        None
    }
}
