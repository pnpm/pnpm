//! Quote text for the scripts that run a package's bins: a POSIX `sh`
//! single-quoted word, and a `cmd` argument whose `%` cannot expand.
//!
//! Creusot proves, see `pnpm/VERIFICATION.md`, that neither function panics,
//! and that what each shell reads back from the result is the text it was
//! given:
//!
//! - `sh` reads the result of [`sh_single_quote`] as one word whose value is
//!   the text.
//! - Every `%` in the result of [`cmd_escape`] is doubled, so `cmd` reads no
//!   variable reference in it and halves each pair back to the text.

#[cfg(creusot)]
pub mod lemmas;
#[cfg(creusot)]
pub mod logic;

#[cfg(creusot)]
use creusot_std::prelude::{
    Seq,
    ensures,
    invariant,
    proof_assert,
    snapshot,
};
#[cfg(creusot)]
use lemmas::{
    cmd_escaped_push,
    cmd_reads_escaped,
    sh_escaped_push,
    sh_reads_quoted,
};
#[cfg(creusot)]
use logic::{
    cmd_escape_char,
    cmd_escaped,
    cmd_read,
    sh_escape,
    sh_escaped,
    sh_read,
};
#[cfg(creusot)]
use pnpm_creusot_models as _;
use split_first_char::SplitFirstChar;

/// Wrap `text` in single quotes for POSIX `sh`, writing each `'` in it as
/// `'\''`. Bin names come from package manifests, so they must not be able
/// to break out of the generated script.
#[cfg_attr(creusot, ensures(sh_read(result@, false) == Some(text@)))]
#[must_use]
pub fn sh_single_quote(text: &str) -> String {
    let mut quoted = String::new();
    quoted.push('\'');
    let mut rest = text;
    #[cfg(creusot)]
    let mut done = snapshot! { Seq::empty() };
    #[cfg(creusot)]
    proof_assert! { text@.concat_empty(); text@ == done.concat(rest@) };
    #[cfg_attr(creusot, invariant(text@ == done.concat(rest@)))]
    #[cfg_attr(creusot, invariant(quoted@ == Seq::singleton('\'').concat(sh_escaped(*done))))]
    while let Some((character, after)) = rest.split_first_char() {
        #[cfg(creusot)]
        proof_assert! {
            sh_escaped_push(*done, character);
            Seq::singleton('\'').concat_assoc(sh_escaped(*done), sh_escape(character));
            sh_escaped(done.concat(Seq::singleton(character)))
                == sh_escaped(*done).concat(sh_escape(character))
        };
        if character == '\'' {
            quoted.push('\'');
            quoted.push('\\');
            quoted.push('\'');
        }
        quoted.push(character);
        #[cfg(creusot)]
        {
            proof_assert! {
                done.concat_assoc(Seq::singleton(character), after@);
                text@ == done.concat(Seq::singleton(character)).concat(after@)
            };
            done = snapshot! { done.concat(Seq::singleton(character)) };
        }
        rest = after;
    }
    #[cfg(creusot)]
    proof_assert! { text@.concat_empty(); rest@.len() == 0 ==> *done == text@ };
    quoted.push('\'');
    #[cfg(creusot)]
    proof_assert! { sh_reads_quoted(text@); sh_read(quoted@, false) == Some(text@) };
    quoted
}

/// Escape `text` for interpolation into a double-quoted `cmd` argument:
/// `%` would otherwise expand as a variable reference.
///
/// `cmd.exe` cannot escape a quote inside a quoted argument at all, so a
/// caller interpolating something other than a file name (which cannot
/// hold one) has to reject quotes before it gets here.
#[cfg_attr(creusot, ensures(cmd_read(result@) == Some(text@)))]
#[must_use]
pub fn cmd_escape(text: &str) -> String {
    let mut escaped = String::new();
    let mut rest = text;
    #[cfg(creusot)]
    let mut done = snapshot! { Seq::empty() };
    #[cfg(creusot)]
    proof_assert! { text@.concat_empty(); text@ == done.concat(rest@) };
    #[cfg_attr(creusot, invariant(text@ == done.concat(rest@)))]
    #[cfg_attr(creusot, invariant(escaped@ == cmd_escaped(*done)))]
    while let Some((character, after)) = rest.split_first_char() {
        #[cfg(creusot)]
        proof_assert! {
            cmd_escaped_push(*done, character);
            cmd_escaped(done.concat(Seq::singleton(character)))
                == cmd_escaped(*done).concat(cmd_escape_char(character))
        };
        if character == '%' {
            escaped.push('%');
        }
        escaped.push(character);
        #[cfg(creusot)]
        {
            proof_assert! {
                done.concat_assoc(Seq::singleton(character), after@);
                text@ == done.concat(Seq::singleton(character)).concat(after@)
            };
            done = snapshot! { done.concat(Seq::singleton(character)) };
        }
        rest = after;
    }
    #[cfg(creusot)]
    proof_assert! { text@.concat_empty(); rest@.len() == 0 ==> *done == text@ };
    #[cfg(creusot)]
    proof_assert! { cmd_reads_escaped(text@); cmd_read(escaped@) == Some(text@) };
    escaped
}

#[cfg(test)]
mod tests;
