use super::StarGlob;
use pretty_assertions::assert_eq;

/// `spec::glob`, written as Rust.
fn glob(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len()).any(|skip| glob(rest, &text[skip..])),
        Some((first, rest)) => text
            .split_first()
            .is_some_and(|(head, tail)| head == first && glob(rest, tail)),
    }
}

/// Every text of up to `longest` characters from `alphabet`.
fn texts(alphabet: &[char], longest: usize) -> Vec<String> {
    let mut all = vec![String::new()];
    let mut shorter = vec![String::new()];
    for _ in 0..longest {
        shorter = shorter
            .iter()
            .flat_map(|text| {
                alphabet
                    .iter()
                    .map(move |character| format!("{text}{character}"))
            })
            .collect();
        all.extend(shorter.iter().cloned());
    }
    all
}

fn agrees_with_the_definition(pattern_alphabet: &[char], input_alphabet: &[char], longest: usize) {
    let inputs = texts(input_alphabet, longest + 1);
    for pattern in texts(pattern_alphabet, longest) {
        let glob_of_pattern = StarGlob::new(&pattern);
        let pattern_chars: Vec<char> = pattern.chars().collect();
        for input in &inputs {
            let input_chars: Vec<char> = input.chars().collect();
            assert_eq!(
                glob_of_pattern.matches(input),
                glob(&pattern_chars, &input_chars),
                "{pattern:?} against {input:?}",
            );
        }
    }
}

#[test]
fn matches_what_the_glob_definition_matches() {
    agrees_with_the_definition(&['a', 'b', '*'], &['a', 'b'], 5);
    agrees_with_the_definition(&['a', 'é', '*'], &['a', 'é'], 4);
}

#[test]
fn matches_the_edges_of_the_walk() {
    let cases = [
        ("foo", "foo", true),
        ("abc", "ab", false),
        ("", "", true),
        ("", "a", false),
        ("*", "", true),
        ("**", "a", true),
        ("a*", "abc", true),
        ("*c", "abc", true),
        ("a*c", "ac", true),
        ("a*b*c", "abxbc", true),
        ("a*b*c", "acb", false),
        // The first and the last segment never overlap.
        ("xy*yx", "xyx", false),
        ("xy*yx", "xyyx", true),
        ("é*é", "é", false),
        ("é*é", "éé", true),
    ];
    for (pattern, input, expected) in cases {
        assert_eq!(
            StarGlob::new(pattern).matches(input),
            expected,
            "{pattern:?} against {input:?}",
        );
    }
}
