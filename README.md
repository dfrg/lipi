# lipi

Lipi (Sanskrit for 'writing, letters, alphabet') is a pure Rust crate that provides
font independent text analysis support for shaping and layout.

### Features

- Constant time access to Unicode character properties with a compact representation
- Character composition and decomposition (canonical and compatible)
- Paragraph level boundary analysis (word and line segmentation)
- Script aware complex cluster parsing
- Abstract iterative method for mapping cluster characters to nominal glyph identifiers
- Basic locale parsing (BCP 47 language tags) with conversions to/from OpenType language tags
