use core::iter::Peekable;
use core::str::Split;

/// Subtag in a locale.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Subtag<'a> {
    /// Primary language subtag.
    Language(&'a str),
    /// Script subtag.
    Script(&'a str),
    /// Region subtag.
    Region(&'a str),
    /// Variant subtag.
    Variant(&'a str),
    /// Extension subtag.
    Extension(&'a str),
    /// Private-use subtag.
    Private(&'a str),
}

/// Returns an iterator that yields subtags of the specified locale.
pub fn subtags<'a>(locale: &'a str) -> Subtags<'a> {
    Subtags {
        stage: ParseStage::Language,
        source: locale,
        parts: locale.split('-').peekable(),
        pos: 0,
    }
}

/// Iterator over the subtags in a locale.
#[derive(Clone)]
pub struct Subtags<'a> {
    stage: ParseStage,
    source: &'a str,
    parts: Peekable<Split<'a, char>>,
    pos: usize,
}

impl<'a> Subtags<'a> {
    /// Returns the remainder of the underlying string.
    pub fn remainder(&self) -> &'a str {
        self.source.get(self.pos..).unwrap_or("")
    }
}

impl<'a> Iterator for Subtags<'a> {
    type Item = Subtag<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let part = self.parts.next()?;
        let part_len = part.len();
        let start = self.pos;
        loop {
            match self.stage {
                ParseStage::Language => {
                    self.stage = ParseStage::Script;
                    match part_len {
                        2 | 3 => {
                            self.pos += part_len + 1;
                            return Some(Subtag::Language(part));
                        }
                        _ => return None,
                    }
                }
                ParseStage::Script => {
                    self.stage = ParseStage::Region;
                    if part_len == 4 && part.as_bytes().iter().all(|ch| ch.is_ascii_alphabetic()) {
                        self.pos += part_len + 1;
                        return Some(Subtag::Script(part));
                    }
                }
                ParseStage::Region => {
                    self.stage = ParseStage::Variant;
                    match part_len {
                        2 => {
                            if part.as_bytes().iter().all(|ch| ch.is_ascii_alphabetic()) {
                                self.pos += part_len + 1;
                                return Some(Subtag::Region(part));
                            }
                        }
                        3 => {
                            if part.as_bytes().iter().all(|ch| ch.is_ascii_digit()) {
                                self.pos += part_len + 1;
                                return Some(Subtag::Region(part));
                            }
                        }
                        _ => {}
                    }
                }
                ParseStage::Variant => match part_len {
                    4 => {
                        if part.as_bytes().iter().enumerate().all(|(i, ch)| {
                            (i == 0 && ch.is_ascii_digit()) || (i > 0 && ch.is_ascii_alphanumeric())
                        }) {
                            self.pos += part_len + 1;
                            return Some(Subtag::Variant(part));
                        } else {
                            return None;
                        }
                    }
                    5..=8 => {
                        if part.as_bytes().iter().all(|ch| ch.is_ascii_alphanumeric()) {
                            self.pos += part_len + 1;
                            return Some(Subtag::Variant(part));
                        } else {
                            return None;
                        }
                    }
                    1 => {
                        self.stage = if part.as_bytes()[0] == b'x' {
                            ParseStage::Private
                        } else {
                            ParseStage::Extension
                        };
                    }
                    _ => return None,
                },
                ParseStage::Extension => {
                    if part_len != 1 {
                        return None;
                    }
                    let mut end = start + part_len + 1;
                    while let Some(subpart) = self.parts.peek() {
                        let subpart_len = subpart.len();
                        match subpart_len {
                            2..=8 => {
                                self.parts.next();
                                end += subpart_len + 1;
                            }
                            1 => {
                                if subpart.as_bytes()[0] == b'x' {
                                    self.stage = ParseStage::Private;
                                }
                                break;
                            }
                            _ => break,
                        }
                    }
                    end = (end - 1).min(self.source.len());
                    let tag = self.source.get(start..end)?;
                    self.pos = end + 1;
                    return Some(Subtag::Extension(tag));
                }
                ParseStage::Private => {
                    if part_len != 1 || part.as_bytes()[0] != b'x' {
                        return None;
                    }
                    let mut end = start + part_len + 1;
                    while let Some(subpart) = self.parts.peek() {
                        let subpart_len = subpart.len();
                        match subpart_len {
                            2..=8 => {
                                self.parts.next();
                                end += subpart_len + 1;
                            }
                            1 => {
                                if subpart.as_bytes()[0] == b'x' {
                                    break;
                                } else {
                                    self.parts.next();
                                    end += subpart_len + 1;
                                }
                            }
                            _ => break,
                        }
                    }
                    end = (end - 1).min(self.source.len());
                    let tag = self.source.get(start..end)?;
                    self.pos = end + 1;
                    return Some(Subtag::Private(tag));
                }
            }
        }
    }
}

#[derive(Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Debug)]
enum ParseStage {
    Language,
    Script,
    Region,
    Variant,
    Extension,
    Private,
}
