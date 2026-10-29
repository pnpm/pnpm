use super::parse;
use std::{
    collections::BTreeMap,
    io::Write,
    process::{Command, Stdio},
};

/// What `node --env-file` would set for each input, read through
/// `util.parseEnv`, which runs the same Node.js parser.
fn parse_with_node(inputs: &[String]) -> Vec<BTreeMap<String, String>> {
    let script = "const { parseEnv } = require('node:util');
const inputs = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
process.stdout.write(JSON.stringify(inputs.map((input) => parseEnv(input))));";
    let mut child = Command::new("node")
        .args(["-e", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn node");
    let stdin = serde_json::to_vec(inputs).expect("serialize inputs");
    child.stdin
        .take()
        .expect("node stdin")
        .write_all(&stdin)
        .expect("write node stdin");
    let output = child.wait_with_output().expect("wait for node");
    assert!(output.status.success(), "node failed: {output:?}");
    serde_json::from_slice(&output.stdout).expect("parse node output")
}

/// Assert that every input parses to what Node.js reads from it.
fn assert_matches_node(inputs: &[String]) {
    let expected = parse_with_node(inputs);
    for (input, expected) in inputs.iter().zip(expected) {
        let actual = parse(input).into_iter().collect::<BTreeMap<_, _>>();
        assert_eq!(actual, expected, "input: {input:?}");
    }
}

#[test]
fn matches_node_on_documented_syntax() {
    let inputs = [
        "T_A=1\nexport T_B=2\nT_C=\"quoted value\"\n",
        "T_A=$HOME ${HOME}\nT_B=\"$HOME\"\n",
        "T_A=C:\\Users\\me\nT_B=\"C:\\Users\\me\"\nT_C=\"C:\\new\"\n",
        "T_A=value # comment\nT_B=value#comment\n# whole-line comment\n",
        "T_A='a\\nb'\nT_B=\"a\\nb\"\nT_C=`a\\nb`\n",
        "T_A=\"line 1\nline 2\"\nT_B=after\n",
        "T_A=hello world\nT_B={\"k\":\"v\"}\nT_C=it's\n",
        "T_A=\nT_B=''\nT_C=\"\"\nT_D=",
        "T_A=1\r\nT_B=\"a\r\nb\"\r\n",
    ];
    assert_matches_node(&inputs.map(String::from));
}

#[test]
fn matches_node_on_malformed_input() {
    let inputs = [
        "no equals here\nT_A=1\n",
        "=value\nT_A=1\n",
        "T_A=\"never closed\nT_B=2\n",
        "T_A='never closed",
        "T_A=\"closed\" trailing\nT_B=2\n",
        "T_A= \nT_B=5\n",
        "export =1\nexport T_A=\nexport\tT_B=2\n",
        "a b=1\nT-C=2\n  T_D = 3  \n",
        "T_A=1\nT_A=2\n",
    ];
    assert_matches_node(&inputs.map(String::from));
}

/// Inputs built from the characters the parser treats specially, so the
/// port is compared with Node.js on combinations no hand-written case
/// covers.
#[test]
fn matches_node_on_generated_input() {
    const ALPHABET: [&str; 16] = [
        "T", "=", r#"""#, "'", "`", "#", "\n", "\r\n", " ", "\t", r"\n", r"\", "$", "x", "export ",
        "é",
    ];
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let inputs = (0..500)
        .map(|_| {
            let len = next() % 24;
            (0..len)
                .map(|_| ALPHABET[usize::try_from(next() % 16).expect("index fits usize")])
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_matches_node(&inputs);
}
