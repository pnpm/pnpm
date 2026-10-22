use super::{
    CWD, Colors, ReporterState, emitted_lines, lifecycle_line, lifecycle_script, render, state,
};
use pnpm_default_reporter::format::visible_width;

const RED: &str = "\u{1b}[31m";
const RESET: &str = "\u{1b}[39m";
const CLEAR_LINE: &str = "\u{1b}[2K";
const CURSOR_UP: &str = "\u{1b}[1A";
const SGR_RESET: &str = "\u{1b}[0m";

fn rendered_output(colors: bool, line: &str) -> String {
    let frame = render(
        &mut state(colors),
        vec![lifecycle_script(CWD, "build", "tsc"), lifecycle_line(CWD, "build", line)],
    );
    let output = frame
        .lines()
        .nth(1)
        .expect("the frame shows the output line");
    output
        .split_once(' ')
        .expect("the output line has a gutter")
        .1
        .to_string()
}

#[test]
fn keeps_colors_of_script_output() {
    let line = format!("{RED}error{RESET} TS2322");
    assert_eq!(rendered_output(true, &line), format!("{line}{SGR_RESET}"));
}

#[test]
fn closes_a_color_left_open_by_script_output() {
    assert_eq!(rendered_output(true, &format!("{RED}error")), format!("{RED}error{SGR_RESET}"));
}

#[test]
fn drops_colors_of_script_output_when_colors_are_off() {
    assert_eq!(rendered_output(false, &format!("{RED}error{RESET} TS2322")), "error TS2322");
}

#[test]
fn drops_cursor_movement_from_script_output() {
    let line = format!("{CURSOR_UP}{CLEAR_LINE}{RED}building{RESET}\u{7}");
    assert_eq!(rendered_output(true, &line), format!("{RED}building{RESET}{SGR_RESET}"));
}

#[test]
fn drops_private_mode_sequences_that_end_like_a_color() {
    let line = format!("\u{1b}[>4;2m{RED}ok{RESET}");
    assert_eq!(rendered_output(true, &line), format!("{RED}ok{RESET}{SGR_RESET}"));
}

#[test]
fn drops_hyperlink_sequences_but_keeps_their_text() {
    let line = "see \u{1b}]8;;https://example.com\u{7}docs\u{1b}]8;;\u{1b}\\ for help";
    assert_eq!(rendered_output(false, line), "see docs for help");
    let line = "see \u{1b}]8;;https://example.com\u{9c}docs\u{1b}]8;;\u{9c} for help";
    assert_eq!(rendered_output(false, line), "see docs for help");
    let line = "see \u{9d}8;;https://example.com\u{9c}docs\u{9d}8;;\u{9c} for help";
    assert_eq!(rendered_output(false, line), "see docs for help");
}

#[test]
fn drops_single_character_csi_sequences() {
    assert_eq!(rendered_output(false, "\u{9b}2Kbuilding"), "building");
}

#[test]
fn shows_the_last_frame_of_a_carriage_return_redraw() {
    assert_eq!(rendered_output(false, "10%\r55%\r100%\r"), "100%");
    assert_eq!(rendered_output(false, &format!("50%\r{CLEAR_LINE}")), "50%");
}

#[test]
fn cuts_colored_output_by_its_visible_width() {
    let line = format!("{RED}{}{RESET}", "x".repeat(100));
    let output = rendered_output(true, &line);
    assert_eq!(visible_width(&output), 78);
    assert_eq!(output, format!("{RED}{}…{RESET}{SGR_RESET}", "x".repeat(77)));
}

#[test]
fn cleans_streamed_script_output() {
    let mut reporter = ReporterState::new(CWD.to_string(), 80, Colors { enabled: false }, true);
    let lines = emitted_lines(
        &mut reporter,
        vec![
            lifecycle_script(CWD, "build", "tsc"),
            lifecycle_line(CWD, "build", &format!("1/2\r{CLEAR_LINE}{RED}2/2{RESET}")),
        ],
    );
    assert_eq!(lines.last().map(String::as_str), Some(". build: 2/2"));
}
