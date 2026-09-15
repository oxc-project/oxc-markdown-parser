//! The reference maps handed from the block phase to the inline phase:
//! link-definition labels and footnote-definition labels, keyed by [`crate::syntax::label::normalize`].
//!
//! Only existence matters here: the parser decides link-vs-literal,
//! while destinations/titles stay on the `Definition` nodes
//! (consumers resolve from the AST with the same normalization).
//!
//! Sorted vecs, not hash sets: keeps the crate dependency-free,
//! and definitions are few enough that binary search wins anyway.
//! [`RefMap::finish`] sorts once, between the block and inline phases.

#[derive(Default)]
pub struct RefMap {
    labels: Vec<String>,
    footnotes: Vec<String>,
}

fn contains(labels: &[String], label: &str) -> bool {
    labels.binary_search_by(|l| l.as_str().cmp(label)).is_ok()
}

impl RefMap {
    pub fn push(&mut self, label: String) {
        self.labels.push(label);
    }

    pub fn finish(&mut self) {
        for labels in [&mut self.labels, &mut self.footnotes] {
            labels.sort_unstable();
            labels.dedup();
        }
    }

    pub fn contains(&self, label: &str) -> bool {
        contains(&self.labels, label)
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    pub fn push_footnote(&mut self, label: String) {
        self.footnotes.push(label);
    }

    pub fn contains_footnote(&self, label: &str) -> bool {
        contains(&self.footnotes, label)
    }

    pub fn footnotes_is_empty(&self) -> bool {
        self.footnotes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_footnotes_are_separate() {
        let mut map = RefMap::default();
        assert!(map.is_empty() && map.footnotes_is_empty());
        map.push("b".into());
        map.push("a".into());
        map.push("a".into());
        map.push_footnote("1".into());
        map.finish();
        assert!(map.contains("a"));
        assert!(!map.contains("1"));
        assert!(map.contains_footnote("1"));
        assert!(!map.contains_footnote("a"));
        assert_eq!(map.labels, ["a", "b"], "storage stays sorted and deduplicated");
    }
}
