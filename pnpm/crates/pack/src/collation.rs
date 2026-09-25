//! The string ordering pnpm sorts packed paths by.

use icu_collator::{Collator, CollatorBorrowed, options::CollatorOptions};
use icu_locale_core::locale;

/// A collator equivalent to JavaScript's `localeCompare(b, 'en')`, which
/// pnpm 11 sorts packed paths with and npm-packlist orders a tarball's
/// entries with. `CollatorOptions::default()` is tertiary strength with
/// punctuation significant, matching `Intl.Collator`'s own defaults of
/// `sensitivity: 'variant'` and `ignorePunctuation: false`.
pub(super) fn en_collator() -> CollatorBorrowed<'static> {
    Collator::try_new(locale!("en").into(), CollatorOptions::default())
        .expect("`en` collation data is compiled into the binary")
}
