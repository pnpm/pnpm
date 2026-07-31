use super::{Collapsed, CollapsingHandler};
use miette::{Diagnostic, MietteHandlerOpts, ReportHandler};
use std::{error::Error, fmt};

/// A `#[diagnostic(transparent)]` wrapper: displays as its inner error
/// and keeps it as the source.
#[derive(Debug)]
struct Wrapper<Inner> {
    inner: Inner,
}

impl<Inner: fmt::Display> fmt::Display for Wrapper<Inner> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner, formatter)
    }
}

impl<Inner: Error + 'static> Error for Wrapper<Inner> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.inner)
    }
}

impl<Inner: Diagnostic + 'static> Diagnostic for Wrapper<Inner> {
    fn code(&self) -> Option<Box<dyn fmt::Display + '_>> {
        self.inner.code()
    }
}

#[derive(Debug)]
struct Leaf {
    message: &'static str,
    source: Option<Box<Leaf>>,
}

impl fmt::Display for Leaf {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for Leaf {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|source| source as &(dyn Error + 'static))
    }
}

impl Diagnostic for Leaf {
    fn code(&self) -> Option<Box<dyn fmt::Display + '_>> {
        Some(Box::new("ERR_PNPM_LEAF"))
    }
}

/// Every line the renderer would print: the head, then one per
/// surviving cause.
fn messages(collapsed: &Collapsed<'_>) -> Vec<String> {
    let mut messages = vec![collapsed.to_string()];
    let mut source = collapsed.source();
    while let Some(cause) = source {
        messages.push(cause.to_string());
        source = cause.source();
    }
    messages
}

#[test]
fn transparent_wrappers_render_the_message_once() {
    let leaf = Leaf { message: "tarball server returned HTTP 404", source: None };
    let wrapped = Wrapper { inner: Wrapper { inner: Wrapper { inner: leaf } } };

    let collapsed = Collapsed::new(&wrapped);

    assert_eq!(messages(&collapsed), vec!["tarball server returned HTTP 404".to_string()]);
}

/// Only *consecutive* restatements fold: a wrapper that says something
/// of its own, and a cause that genuinely repeats an earlier message
/// further down the chain, both stay in the rendering.
#[test]
fn distinct_causes_survive() {
    let leaf = Leaf {
        message: "installing dependencies",
        source: Some(Box::new(Leaf {
            message: "tarball server returned HTTP 404",
            source: Some(Box::new(Leaf { message: "installing dependencies", source: None })),
        })),
    };
    let wrapped = Wrapper { inner: leaf };

    let collapsed = Collapsed::new(&wrapped);

    assert_eq!(
        messages(&collapsed),
        vec![
            "installing dependencies".to_string(),
            "tarball server returned HTTP 404".to_string(),
            "installing dependencies".to_string(),
        ],
    );
}

/// A wrapper that prefixes its cause with context repeats it in full,
/// so the cause folds into the wrapper's line.
#[test]
fn a_context_prefixed_wrapper_absorbs_its_cause() {
    let leaf = Leaf {
        message: "Failed to resolve dependency tree: No matching version found for is-odd@99.99.99",
        source: Some(Box::new(Leaf {
            message: "No matching version found for is-odd@99.99.99",
            source: None,
        })),
    };

    let collapsed = Collapsed::new(&leaf);

    assert_eq!(
        messages(&collapsed),
        vec![
            "Failed to resolve dependency tree: No matching version found for is-odd@99.99.99"
                .to_string(),
        ],
    );
}

/// A cause is only absorbed when the wrapper appended it behind a
/// separator. A tail that matches mid-token is a coincidence, and the
/// cause survives.
#[test]
fn a_coincidental_tail_match_is_not_a_restatement() {
    let leaf = Leaf {
        message: "the range resolved to 3.0.1",
        source: Some(Box::new(Leaf { message: "0.1", source: None })),
    };

    let collapsed = Collapsed::new(&leaf);

    assert_eq!(
        messages(&collapsed),
        vec!["the range resolved to 3.0.1".to_string(), "0.1".to_string()],
    );
}

/// The kept levels keep answering for themselves — the collapsed head
/// still carries the code miette prints above the report.
#[test]
fn the_head_keeps_its_diagnostic_code() {
    let wrapped = Wrapper { inner: Leaf { message: "boom", source: None } };

    let collapsed = Collapsed::new(&wrapped);

    assert_eq!(collapsed.code().map(|code| code.to_string()), Some("ERR_PNPM_LEAF".to_string()));
}

/// A wrapper that offers its inner error as a *diagnostic* source.
/// miette prefers that over the plain error source, so the fold has to
/// descend it too.
#[derive(Debug)]
struct DiagnosticWrapper {
    inner: Leaf,
}

impl fmt::Display for DiagnosticWrapper {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner, formatter)
    }
}

impl Error for DiagnosticWrapper {}

impl Diagnostic for DiagnosticWrapper {
    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        Some(&self.inner)
    }
}

#[test]
fn a_diagnostic_source_is_folded_like_an_error_source() {
    let wrapped = DiagnosticWrapper {
        inner: Leaf { message: "tarball server returned HTTP 404", source: None },
    };

    let collapsed = Collapsed::new(&wrapped);

    assert_eq!(messages(&collapsed), vec!["tarball server returned HTTP 404".to_string()]);
}

/// Renders through the installed handler, which is the only thing the
/// CLI ever calls. `format!("{:?}")` is how miette's `Report` reaches
/// it.
#[test]
fn the_handler_renders_a_repeated_message_once() {
    struct Rendered<'a>(&'a CollapsingHandler, &'a dyn Diagnostic);

    impl fmt::Debug for Rendered<'_> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.debug(self.1, formatter)
        }
    }

    let handler = CollapsingHandler { inner: MietteHandlerOpts::new().build() };
    let wrapped = Wrapper { inner: Wrapper { inner: Leaf { message: "boom", source: None } } };

    let rendered = format!("{:?}", Rendered(&handler, &wrapped));

    assert_eq!(
        rendered.matches("boom").count(),
        1,
        "the message must be rendered once, got:\n{rendered}",
    );
}
