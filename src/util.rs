//! Shared, dependency-free text helpers (ADR-004).
//!
//! A leaf module so both `config` (did-you-mean hints on load errors) and
//! `skills` (name suggestions) share one Levenshtein implementation without a
//! `config → skills` layering inversion. Keep this module free of `config`/
//! `skills` types so it stays a pure leaf.

/// Levenshtein edit distance between two strings, compared character-wise.
/// Moved verbatim from `skills` so suggestion behavior stays byte-identical.
pub fn edit_distance(left: &str, right: &str) -> usize {
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut current = vec![0; right_chars.len() + 1];

    for (left_index, left_character) in left.chars().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_character) in right_chars.iter().enumerate() {
            let substitution_cost = usize::from(left_character != *right_character);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + substitution_cost);
        }
        previous.clone_from(&current);
    }

    previous[right_chars.len()]
}

/// The single closest known name within an edit-distance threshold
/// (`2.max(unknown.len() / 3)`, matching the skills convention), or `None` when
/// no candidate is close enough. Ties resolve to the first minimum encountered,
/// so the result is deterministic for a given iteration order.
pub fn suggest_nearby_name<'a>(
    unknown: &str,
    known: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let max = 2.max(unknown.len() / 3);
    known
        .into_iter()
        .map(|candidate| (edit_distance(unknown, candidate), candidate))
        .filter(|(distance, _)| *distance <= max)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_counts_single_and_multi_edits() {
        assert_eq!(edit_distance("codx", "codex"), 1);
        assert_eq!(edit_distance("codex", "codex"), 0);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
    }

    #[test]
    fn suggest_nearby_name_returns_the_closest_within_threshold() {
        assert_eq!(
            suggest_nearby_name("codx", ["codex", "claude", "cursor", "zai"]),
            Some("codex")
        );
    }

    #[test]
    fn suggest_nearby_name_returns_none_beyond_threshold() {
        assert_eq!(
            suggest_nearby_name("zzzzzz", ["codex", "claude", "cursor", "zai"]),
            None
        );
    }

    #[test]
    fn suggest_nearby_name_picks_the_smallest_distance_among_candidates() {
        // Both "fixer" (distance 1) and "mixer" (distance 1)… here only "fixer"
        // is within reach of "fixe" at distance 1, while "fixture" is farther.
        assert_eq!(
            suggest_nearby_name("fixe", ["fixer", "fixture"]),
            Some("fixer")
        );
        // When two candidates tie on the closest distance, one of them is chosen
        // deterministically (first minimum in iteration order).
        assert_eq!(
            suggest_nearby_name("fixer", ["fixed", "mixer"]),
            Some("fixed")
        );
    }
}
