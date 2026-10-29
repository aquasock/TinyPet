//! Parser for the `.LNZ` breed files.
//!
//! These are the authoritative description of a pet: shape overrides, per-ball
//! colour, fuzz, outline and texture ids. They are plain text with `[...]`
//! section headers and `;` comments - the format the community has hand-edited
//! for twenty-five years.
//!
//! This parser deliberately keeps the raw lines. The numeric interpretation of
//! each section is content-specific, and guessing it here would hide mistakes
//! rather than surface them.

#[derive(Clone, Debug)]
pub struct Section {
    /// Section name without the brackets.
    pub name: String,
    /// Text after `;` on the header line, if any.
    pub comment: Option<String>,
    /// Body lines, verbatim, excluding the header.
    pub lines: Vec<String>,
}

impl Section {
    /// Body lines that carry data: non-empty and not a comment.
    pub fn data_lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with(';'))
    }

    /// Split a data line into numeric fields, dropping trailing labels.
    ///
    /// LNZ rows look like `35, 244, 244, 1, -1, 3, 4, 1   eBall_ankleL,` - comma
    /// separated numbers followed by a name. This returns the numbers.
    pub fn numbers(&self) -> Vec<Vec<i32>> {
        self.data_lines()
            .map(|line| {
                line.split(|c: char| c == ',' || c.is_whitespace())
                    .filter_map(|f| f.trim().parse::<i32>().ok())
                    .collect()
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Lnz {
    sections: Vec<Section>,
}

impl Lnz {
    pub fn parse(text: &str) -> Self {
        let mut sections: Vec<Section> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim_end_matches(['\r', '\n']);
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix('[') {
                if let Some(close) = rest.find(']') {
                    let name = rest[..close].trim().to_string();
                    let tail = rest[close + 1..].trim();
                    let comment = tail.strip_prefix(';').map(|c| c.trim().to_string());
                    sections.push(Section { name, comment, lines: Vec::new() });
                    continue;
                }
            }
            if let Some(last) = sections.last_mut() {
                last.lines.push(line.to_string());
            }
        }
        Self { sections }
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    pub fn section_names(&self) -> impl Iterator<Item = &str> {
        self.sections.iter().map(|s| s.name.as_str())
    }

    pub fn len(&self) -> usize { self.sections.len() }
    pub fn is_empty(&self) -> bool { self.sections.is_empty() }
}
