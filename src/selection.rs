pub fn select<'a>(
    files: &'a [String],
    previous: Option<&str>,
    mut random: impl FnMut() -> u32,
) -> Option<&'a str> {
    let candidates: Vec<&str> = files
        .iter()
        .map(String::as_str)
        .filter(|file| files.len() == 1 || Some(*file) != previous)
        .collect();
    let count = u32::try_from(candidates.len()).ok()?;
    if count == 0 {
        return None;
    }
    let threshold = count.wrapping_neg() % count;
    let value = loop {
        let value = random();
        if value >= threshold {
            break value;
        }
    };
    Some(candidates[(value % count) as usize])
}

#[cfg(test)]
#[allow(dead_code, unused_imports)]
mod tests {
    use super::select;

    #[test]
    fn selects_each_candidate_without_repeating_the_previous_file() {
        let files = vec!["first".into(), "bird.wav".into(), "third".into()];
        assert_eq!(select(&files, Some("first"), || 0), Some("bird.wav"));
        assert_eq!(select(&files, Some("first"), || 1), Some("third"));
    }

    #[test]
    fn handles_empty_and_single_file() {
        assert_eq!(select(&[], None, || 0), None);
        assert_eq!(
            select(&["anything".into()], Some("anything"), || 0),
            Some("anything")
        );
    }

    #[test]
    fn rejects_values_that_would_bias_selection() {
        let files = vec!["first".into(), "second".into(), "third".into()];
        let mut values = [0, 3].into_iter();
        assert_eq!(
            select(&files, None, || values.next().unwrap()),
            Some("first")
        );
        assert_eq!(values.next(), None);
    }
}
