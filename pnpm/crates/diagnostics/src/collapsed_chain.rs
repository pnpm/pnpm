//! Report handler that renders each distinct cause once.
//!
//! pacquet's error enums wrap one another with
//! `#[diagnostic(transparent)]` variants so `?` composes across crate
//! boundaries. Such a variant displays as its inner error *and* keeps
//! it as its `source`, so a leaf error four wrappers deep renders the
//! same sentence at five levels of miette's `├─▶` cause chain. The
//! handler here folds those levels away before delegating to miette's
//! own renderer, which keeps every theme, width, and colour decision
//! miette would otherwise make.

use miette::{
    Diagnostic, LabeledSpan, MietteHandler, MietteHandlerOpts, ReportHandler, Severity, SourceCode,
};
use std::{error::Error, fmt};

/// Route [`miette::Report`] rendering through the collapsing handler.
///
/// A [`miette::Report`] captures the installed hook when it is built,
/// so this has to run before the first one is created. A hook that is
/// already installed is left alone — the first caller wins, and the
/// only caller is the CLI entry point.
pub fn install_report_handler() {
    let _ = miette::set_hook(Box::new(|_| {
        Box::new(CollapsingHandler { inner: MietteHandlerOpts::new().build() })
    }));
}

struct CollapsingHandler {
    inner: MietteHandler,
}

impl ReportHandler for CollapsingHandler {
    fn debug(
        &self,
        diagnostic: &dyn Diagnostic,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        self.inner.debug(&Collapsed::new(diagnostic), formatter)
    }
}

/// The reported error, with consecutive repeats of the same message
/// dropped from its cause chain.
///
/// Every [`Diagnostic`] method delegates to the reported error itself,
/// so its code, help, labels, and source snippet render unchanged.
/// Only the chain below it is ours.
struct Collapsed<'a> {
    head: &'a dyn Diagnostic,
    causes: Option<Box<Cause>>,
}

/// One surviving level below the head, holding its rendered message.
///
/// Owned rather than borrowed because [`Error::source`] hands the
/// renderer a `&(dyn Error + 'static)`, which a chain borrowed from
/// the report could not satisfy. A level's message is all miette
/// prints for it — the causes below the head render as text, not as
/// nested reports.
#[derive(Debug)]
struct Cause {
    message: String,
    next: Option<Box<Cause>>,
}

impl<'a> Collapsed<'a> {
    fn new(head: &'a dyn Diagnostic) -> Self {
        let mut messages = Vec::new();
        let mut last = head.to_string();
        let mut level = nested(Level::Diagnostic(head));
        while let Some(current) = level {
            let message = current.to_string();
            if message != last {
                messages.push(message.clone());
                last = message;
            }
            level = nested(current);
        }

        let mut causes = None;
        for message in messages.into_iter().rev() {
            causes = Some(Box::new(Cause { message, next: causes }));
        }
        Collapsed { head, causes }
    }
}

/// One level of the reported error's cause chain. miette walks a mix
/// of [`Diagnostic`]s and plain [`Error`]s; both shapes descend
/// differently, so both are tracked.
#[derive(Clone, Copy)]
enum Level<'a> {
    Diagnostic(&'a dyn Diagnostic),
    Error(&'a (dyn Error + 'static)),
}

/// The next level down, picked the way miette itself descends: a
/// diagnostic source when the level offers one, its plain error source
/// otherwise.
fn nested(level: Level<'_>) -> Option<Level<'_>> {
    match level {
        Level::Diagnostic(diagnostic) => diagnostic
            .diagnostic_source()
            .map(Level::Diagnostic)
            .or_else(|| diagnostic.source().map(Level::Error)),
        Level::Error(error) => error.source().map(Level::Error),
    }
}

impl fmt::Display for Level<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Level::Diagnostic(diagnostic) => fmt::Display::fmt(diagnostic, formatter),
            Level::Error(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl fmt::Display for Collapsed<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.head, formatter)
    }
}

impl fmt::Debug for Collapsed<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.head, formatter)
    }
}

impl Error for Collapsed<'_> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.causes.as_deref().map(|cause| cause as &(dyn Error + 'static))
    }
}

impl Diagnostic for Collapsed<'_> {
    fn code(&self) -> Option<Box<dyn fmt::Display + '_>> {
        self.head.code()
    }

    fn severity(&self) -> Option<Severity> {
        self.head.severity()
    }

    fn help(&self) -> Option<Box<dyn fmt::Display + '_>> {
        self.head.help()
    }

    fn url(&self) -> Option<Box<dyn fmt::Display + '_>> {
        self.head.url()
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.head.source_code()
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        self.head.labels()
    }

    fn related(&self) -> Option<Box<dyn Iterator<Item = &dyn Diagnostic> + '_>> {
        self.head.related()
    }
}

impl fmt::Display for Cause {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for Cause {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.next.as_deref().map(|cause| cause as &(dyn Error + 'static))
    }
}

#[cfg(test)]
mod tests;
