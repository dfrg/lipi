use core::ops::Range;

#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub(crate) struct Range32 {
    pub(crate) start: u32,
    pub(crate) end: u32,
}

impl Range32 {
    pub(crate) const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub(crate) fn from_usize(range: Range<usize>) -> Self {
        Self {
            start: range.start as u32,
            end: range.end as u32,
        }
    }

    pub(crate) fn to_usize(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.start == self.end
    }
}
