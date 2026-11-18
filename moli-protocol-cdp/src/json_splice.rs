//! pilot: rewrite a few members of a CDP message's JSON text in place.
//!
//! A Runtime.evaluate can carry a 346 KB expression (Playwright's injected
//! script). Rewriting its `id` by parsing the command into a `Value` and
//! serializing it again re-encodes every byte of that expression. These
//! helpers replace only the bytes of the members that change: a scanning
//! visitor reads the members as borrowed [`RawValue`]s, whose text is a slice
//! of the command, and [`JsonSplice`] copies the rest of the command verbatim.

use std::ops::Range;

use serde_json::value::RawValue;

/// The byte range `raw` occupies in `json`, when `raw` was read borrowed from
/// `json` (by `serde_json::from_str` into a `&RawValue`). `None` when it was
/// not.
pub fn raw_value_span(json: &str, raw: &RawValue) -> Option<Range<usize>> {
    let text = raw.get();
    let start = text.as_ptr().addr().checked_sub(json.as_ptr().addr())?;
    let end = start.checked_add(text.len())?;
    let slice = json.get(start..end)?;
    std::ptr::eq(slice.as_ptr(), text.as_ptr()).then_some(start..end)
}

/// Replacements of byte ranges of one JSON text, applied in one copy.
pub struct JsonSplice<'a> {
    json: &'a str,
    edits: Vec<(Range<usize>, String)>,
}

impl<'a> JsonSplice<'a> {
    pub fn new(json: &'a str) -> Self {
        Self {
            json,
            edits: Vec::new(),
        }
    }

    /// Replaces the bytes of `span` with `text`.
    pub fn replace(&mut self, span: Range<usize>, text: String) {
        self.edits.push((span, text));
    }

    /// Inserts `text` before the byte at `at`.
    pub fn insert(&mut self, at: usize, text: String) {
        self.edits.push((at..at, text));
    }

    /// The text with every edit applied, or `None` when two edits overlap or
    /// an edit does not fall on character boundaries of the text.
    pub fn finish(mut self) -> Option<String> {
        self.edits.sort_by_key(|(span, _)| (span.start, span.end));
        let added = self.edits.iter().map(|(_, text)| text.len()).sum::<usize>();
        let mut spliced = String::with_capacity(self.json.len() + added);
        let mut copied_up_to = 0;
        for (span, text) in &self.edits {
            if span.start < copied_up_to || self.json.get(span.clone()).is_none() {
                return None;
            }
            spliced.push_str(self.json.get(copied_up_to..span.start)?);
            spliced.push_str(text);
            copied_up_to = span.end;
        }
        spliced.push_str(self.json.get(copied_up_to..)?);
        Some(spliced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Borrowed<'a> {
        #[serde(borrow)]
        id: &'a RawValue,
    }

    // 2026-11-18: the frontend router and moli-protocol rewrote a command's id
    // by re-serializing all of it; the span of a borrowed member is what lets
    // them replace the id alone.
    #[test]
    fn raw_value_span_locates_a_borrowed_member_in_its_text() {
        let json = r#"{ "params": {"id": 1}, "id" :  42 , "x": "\"id\": 3" }"#;
        let borrowed = serde_json::from_str::<Borrowed<'_>>(json).expect("borrowed id");
        let span = raw_value_span(json, borrowed.id).expect("span of the id");
        assert_eq!(&json[span], "42");

        let elsewhere = serde_json::from_str::<Box<RawValue>>("42").expect("owned value");
        assert_eq!(raw_value_span(json, &elsewhere), None);
    }

    #[test]
    fn json_splice_replaces_and_inserts_without_touching_the_rest() {
        let json = r#"{"id":7,"params":{"expression":"a\"b\\c\u0041"}}"#;
        let mut splice = JsonSplice::new(json);
        splice.replace(6..7, "91".to_owned());
        splice.insert(7, r#","sessionId":"S""#.to_owned());
        assert_eq!(
            splice.finish().as_deref(),
            Some(r#"{"id":91,"sessionId":"S","params":{"expression":"a\"b\\c\u0041"}}"#)
        );

        let mut overlapping = JsonSplice::new(json);
        overlapping.replace(1..7, "x".to_owned());
        overlapping.replace(6..8, "y".to_owned());
        assert_eq!(overlapping.finish(), None);

        assert_eq!(JsonSplice::new(json).finish().as_deref(), Some(json));
    }
}
