use super::Collapsed;
use miette::Diagnostic;
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

/// Only *consecutive* repeats fold: a wrapper that adds its own
/// context, and a cause that genuinely repeats an earlier message
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

/// The kept levels keep answering for themselves — the collapsed head
/// still carries the code miette prints above the report.
#[test]
fn the_head_keeps_its_diagnostic_code() {
    let wrapped = Wrapper { inner: Leaf { message: "boom", source: None } };

    let collapsed = Collapsed::new(&wrapped);

    assert_eq!(collapsed.code().map(|code| code.to_string()), Some("ERR_PNPM_LEAF".to_string()));
}
