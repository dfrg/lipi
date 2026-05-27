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

#[test]
fn bidi_test() {
    let mut state = TestState::new();
    let file = File::open("tests/BidiTest.txt").unwrap();
    let reader = BufReader::new(file);
    let mut codepoints = Vec::new();
    let mut levels = Vec::new();
    let mut order = Vec::new();
    for line in reader.lines() {
        let line = line.unwrap();
        let line = line.trim();
        if line.is_empty() || line.starts_with("#") {
            continue;
        }
        if line.starts_with("@Levels:") {
            levels.clear();
            for level in line.trim_start_matches("@Levels:\t").split(" ") {
                levels.push(level.trim().to_owned());
            }
            continue;
        }
        if line.starts_with("@Reorder:") {
            order.clear();
            let line = line[9..].trim();
            if line.is_empty() {
                continue;
            }
            for ord in line.split(" ") {
                let ord = ord.trim();
                if ord.is_empty() {
                    continue;
                }
                order.push(u32::from_str_radix(ord, 10).unwrap() as usize);
            }
            continue;
        }
        codepoints.clear();
        let mut step = 0;
        let mut dirs = 0;
        for part in line.split("; ") {
            match step {
                0 => {
                    for ty in part.split(" ") {
                        codepoints.push(char_from_type(ty));
                    }
                    step += 1;
                }
                1 => {
                    dirs = u32::from_str_radix(part.trim(), 16).unwrap();
                    step += 1;
                }
                _ => break,
            }
        }
        state.run_dirs(&codepoints, &levels, &order, dirs as u8);
    }
    state.finish();
}

#[test]
fn bidi_character_test() {
    let mut state = TestState::new();
    let file = File::open("tests/BidiCharacterTest.txt").unwrap();
    let reader = BufReader::new(file);
    let mut codepoints = Vec::new();
    let mut levels = Vec::new();
    let mut order = Vec::new();
    for line in reader.lines() {
        let line = line.unwrap();
        let line = line.trim();
        if line.is_empty() || line.starts_with("#") {
            continue;
        }
        codepoints.clear();
        levels.clear();
        let mut dir = None;
        let mut step = 0;
        let mut base_level = 0;
        for part in line.split(";") {
            match step {
                0 => {
                    for codepoint in part.split(" ") {
                        codepoints.push(unsafe {
                            std::char::from_u32_unchecked(
                                u32::from_str_radix(codepoint, 16).unwrap(),
                            )
                        });
                    }
                    step += 1;
                }
                1 => {
                    match part {
                        "0" => dir = Some(0),
                        "1" => dir = Some(1),
                        _ => dir = None,
                    }
                    step += 1;
                }
                2 => {
                    base_level = u32::from_str_radix(part, 10).unwrap() as u8;
                    step += 1;
                }
                3 => {
                    for level in part.trim().split(" ") {
                        levels.push(level.trim().to_owned());
                    }
                    step += 1;
                }
                4 => {
                    order.clear();
                    for ord in part.trim().split(" ") {
                        order.push(u32::from_str_radix(ord, 10).unwrap() as usize);
                    }
                }
                _ => {}
            }
        }
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
            resolver: BidiResolver::new(),
            failures: Vec::new(),
            count: 0,
            failure_count: 0,
        }
    }

    fn run_dirs(&mut self, codepoints: &[char], levels: &[String], order: &[usize], dirs: u8) {
        if dirs & 1 != 0 {
            self.run(None, codepoints, levels, order, None);
        }
        if dirs & 2 != 0 {
            self.run(None, codepoints, levels, order, Some(0));
        }
        if dirs & 4 != 0 {
            self.run(None, codepoints, levels, order, Some(1));
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
        self.resolver.resolve_trailing_neutrals(&classes);
        let test_base_level = self.resolver.base_level();
        let test_levels = self.resolver.levels();
        let test_levels_str = test_levels
            .iter()
            .enumerate()
            .map(|(i, level)| {
                if is_removed_by_x9(classes[i]) {
                    "x".to_owned()
                } else {
                    format!("{}", *level)
                }
            })
            .collect::<Vec<_>>();
        let mut test_order = vec![0; test_levels.len()];
        _reorder(&mut test_order, |i| test_levels[i]);
        test_order.retain(|i| !is_removed_by_x9(classes[*i]));
        if test_levels_str != levels
            || test_order != order
            || (expected_base_level != None && expected_base_level != Some(test_base_level))
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
