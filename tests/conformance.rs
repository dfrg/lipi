extern crate alloc;

use bidi::*;
use icu_properties::props::{BidiMirroringGlyph, BidiPairedBracketType, EnumeratedProperty};
use std::fs::File;
use std::io::{BufRead, BufReader};

// Pull in the bidi module from src/text/bidi.rs, which is not public and thus not accessible from the tests directory.
#[path = "../src/text/bidi.rs"]
mod bidi;

impl BidiClass {
    fn from_char(ch: char) -> Self {
        BidiClass::from_icu4c_value(icu_properties::props::BidiClass::for_char(ch).to_icu4c_value())
    }
}

fn bidi_bracket_from_icu(ch: char) -> Option<BidiBracket> {
    let bracket = BidiMirroringGlyph::for_char(ch);
    match bracket.paired_bracket_type {
        BidiPairedBracketType::Open => bracket.mirroring_glyph.map(BidiBracket::Open),
        BidiPairedBracketType::Close => bracket.mirroring_glyph.map(BidiBracket::Close),
        BidiPairedBracketType::None => None,
        _ => None,
    }
}

fn parse_usize_list(input: &str) -> Vec<usize> {
    input
        .split_whitespace()
        .map(|s| s.parse::<usize>().unwrap())
        .collect()
}

fn parse_level_list(input: &str) -> Vec<String> {
    input.split_whitespace().map(str::to_owned).collect()
}

fn resolve_trailing_neutrals(levels: &mut [u8], classes: &[BidiClass], base_level: u8) {
    for i in (0..classes.len()).rev() {
        let class = classes[i];
        if is_removed_by_x9(class) {
            continue;
        }
        if needs_trailing_neutral_reset(class) {
            levels[i] = base_level;
        } else {
            break;
        }
    }
}

fn test_data_lines(path: &str) -> impl Iterator<Item = String> {
    let file = File::open(path).unwrap();
    let reader = BufReader::new(file);
    reader
        .lines()
        .map(|line| line.unwrap().trim().to_owned())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

#[test]
fn bidi_test() {
    let mut state = TestState::new();
    let mut codepoints = Vec::new();
    let mut levels = Vec::new();
    let mut order = Vec::new();
    for line in test_data_lines("tests/BidiTest.txt") {
        if let Some(rest) = line.strip_prefix("@Levels:\t") {
            levels = parse_level_list(rest);
            continue;
        }
        if let Some(rest) = line.strip_prefix("@Reorder:") {
            order = parse_usize_list(rest);
            continue;
        }
        codepoints.clear();
        let (types, dirs_hex) = line.split_once("; ").unwrap();
        codepoints.extend(types.split_whitespace().map(char_from_type));
        let dirs = u8::from_str_radix(dirs_hex.trim(), 16).unwrap();
        state.run_dirs(&codepoints, &levels, &order, dirs);
    }
    state.finish();
}

#[test]
fn bidi_character_test() {
    let mut state = TestState::new();
    for line in test_data_lines("tests/BidiCharacterTest.txt") {
        let parts = line.split(';').collect::<Vec<_>>();
        // BidiCharacterTest fields:
        // [0] code points, [1] paragraph direction hint, [2] resolved base level,
        // [3] expected levels, [4] expected visual reorder.
        assert_eq!(parts.len(), 5, "invalid BidiCharacterTest line: {line}");
        let codepoints = parts[0]
            .split_whitespace()
            .map(|codepoint| {
                let cp = u32::from_str_radix(codepoint, 16).unwrap();
                char::from_u32(cp).unwrap()
            })
            .collect::<Vec<char>>();
        let dir = match parts[1].trim() {
            "0" => Some(0),
            "1" => Some(1),
            _ => None,
        };
        let base_level = parts[2].trim().parse::<u8>().unwrap();
        let levels = parse_level_list(parts[3]);
        let order = parse_usize_list(parts[4]);
        state.run(Some(base_level), &codepoints, &levels, &order, dir);
    }
    state.finish();
}

struct TestState {
    resolver: BidiResolver,
    failures: Vec<Failure>,
    count: usize,
    failure_count: usize,
}

impl TestState {
    fn new() -> Self {
        Self {
            resolver: BidiResolver::default(),
            failures: Vec::new(),
            count: 0,
            failure_count: 0,
        }
    }

    fn run_dirs(&mut self, codepoints: &[char], levels: &[String], order: &[usize], dirs: u8) {
        for (mask, base_level) in [(1, None), (2, Some(0)), (4, Some(1))] {
            if dirs & mask != 0 {
                self.run(None, codepoints, levels, order, base_level);
            }
        }
    }

    fn run(
        &mut self,
        expected_base_level: Option<u8>,
        codepoints: &[char],
        levels: &[String],
        order: &[usize],
        input_base_level: Option<u8>,
    ) {
        let index = self.count;
        self.count += 1;
        let classes = codepoints
            .iter()
            .copied()
            .map(BidiClass::from_char)
            .collect::<Vec<_>>();
        let brackets = codepoints
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(i, ch)| bidi_bracket_from_icu(ch).map(|b| (i, ch, b)))
            .collect::<Vec<_>>();
        self.resolver.resolve(&classes, &brackets, input_base_level);
        let test_base_level = self.resolver.base_level();
        let mut test_levels = self.resolver.levels().to_vec();
        resolve_trailing_neutrals(&mut test_levels, &classes, test_base_level);
        let test_levels_str = test_levels
            .iter()
            .enumerate()
            .map(|(i, level)| {
                if is_removed_by_x9(classes[i]) {
                    "x".to_owned()
                } else {
                    level.to_string()
                }
            })
            .collect::<Vec<_>>();
        let mut test_order = vec![0; test_levels.len()];
        _reorder(&mut test_order, |i| test_levels[i]);
        test_order.retain(|i| !is_removed_by_x9(classes[*i]));
        if test_levels_str != levels
            || test_order != order
            || expected_base_level.is_some_and(|expected| expected != test_base_level)
        {
            self.failure_count += 1;
            if self.failure_count <= 25 {
                self.failures.push(Failure {
                    index,
                    codepoints: codepoints.to_owned(),
                    exp_levels: levels.to_owned(),
                    levels: test_levels_str,
                    exp_order: order.to_owned(),
                    order: test_order,
                    exp_base_level: expected_base_level,
                    base_level: test_base_level,
                });
            }
        }
    }

    fn finish(&self) {
        if self.failure_count != 0 {
            panic!(
                "{}/{} passed, {} failed\n{:?}",
                self.count - self.failure_count,
                self.count,
                self.failure_count,
                &self.failures
            );
        }
    }
}

#[derive(Debug)]
// Fields are only read by the Debug impl when printing
// failures and rustc ignores those uses.
#[allow(dead_code)]
struct Failure {
    index: usize,
    codepoints: Vec<char>,
    exp_levels: Vec<String>,
    levels: Vec<String>,
    exp_order: Vec<usize>,
    order: Vec<usize>,
    exp_base_level: Option<u8>,
    base_level: u8,
}

fn char_from_type(ty: &str) -> char {
    core::char::from_u32(match ty {
        "ON" => '|' as u32,
        "L" => 0x200E,
        "R" => 0x200F,
        "AN" => 0x661,
        "EN" => '0' as u32,
        "AL" => 0x61C,
        "NSM" => 0x300,
        "CS" => ',' as u32,
        "ES" => '+' as u32,
        "ET" => '$' as u32,
        "BN" => 3,
        "S" => '\t' as u32,
        "WS" => ' ' as u32,
        "B" => '\n' as u32,
        "RLO" => 0x202E,
        "RLE" => 0x202B,
        "LRO" => 0x202D,
        "LRE" => 0x202A,
        "PDF" => 0x202C,
        "FSI" => 0x2068,
        "LRI" => 0x2066,
        "PDI" => 0x2069,
        "RLI" => 0x2067,
        _ => 0,
    })
    .unwrap()
}
