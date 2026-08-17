//! The reference maps handed from the block phase to the inline phase:
//! link-definition labels and footnote-definition labels, keyed by [`crate::syntax::label::normalize`].
//!
//! Only existence matters here: the parser decides link-vs-literal,
//! while destinations/titles stay on the `Definition` nodes
//! (consumers resolve from the AST with the same normalization).
//!
//! Sorted vecs, not hash sets: keeps the crate dependency-free,
//! and definitions are few enough that binary search wins anyway.

#[derive(Default)]
pub struct RefMap {
    labels: Vec<String>,
    footnotes: Vec<String>,
}

fn insert_sorted(labels: &mut Vec<String>, label: String) {
    if let Err(at) = labels.binary_search(&label) {
        labels.insert(at, label);
    }
}

fn contains(labels: &[String], label: &str) -> bool {
    labels.binary_search_by(|l| l.as_str().cmp(label)).is_ok()
}

impl RefMap {
    pub fn insert(&mut self, label: String) {
        insert_sorted(&mut self.labels, label);
    }

    pub fn contains(&self, label: &str) -> bool {
        contains(&self.labels, label)
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    pub fn insert_footnote(&mut self, label: String) {
        insert_sorted(&mut self.footnotes, label);
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
        map.insert("b".into());
        map.insert("a".into());
        map.insert("a".into());
        map.insert_footnote("1".into());
        assert!(map.contains("a"));
        assert!(!map.contains("1"));
        assert!(map.contains_footnote("1"));
        assert!(!map.contains_footnote("a"));
        assert_eq!(map.labels, ["a", "b"], "storage stays sorted and deduplicated");
    }
}
