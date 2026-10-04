use std::collections::HashMap;

/// Counts word frequencies in `text`.
pub fn word_counts(text: &str) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1; // tally
    }
    counts
}
