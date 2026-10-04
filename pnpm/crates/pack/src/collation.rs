//! The string ordering pnpm sorts packed paths by.

use icu_collator::{
    Collator,
    options::CollatorOptions,
    provider::{
        CollationDiacriticsV1, CollationJamoV1, CollationMetadataV1, CollationReorderingV1,
        CollationRootV1, CollationSpecialPrimariesV1, CollationTailoringV1,
    },
};
use icu_locale_core::locale;
use icu_normalizer::provider::{NormalizerNfdDataV1, NormalizerNfdTablesV1};
use icu_provider::{DataError, DataErrorKind, DataMarker, DataProvider, DataRequest, DataResponse};

/// A collator equivalent to JavaScript's `localeCompare(b, 'en')`, which
/// pnpm 11 sorts packed paths with and npm-packlist orders a tarball's
/// entries with. `CollatorOptions::default()` is tertiary strength with
/// punctuation significant, matching `Intl.Collator`'s own defaults of
/// `sensitivity: 'variant'` and `ignorePunctuation: false`.
pub(super) fn en_collator() -> Collator {
    Collator::try_new_unstable(&RootCollation, locale!("en").into(), CollatorOptions::default())
        .expect("`en` uses root collation, which is compiled into the binary")
}

/// ICU4X's compiled collation data without the per-locale tailorings,
/// which are most of its size. `en` has no tailoring: the collator loads
/// one only for a locale whose metadata says it is tailored.
struct RootCollation;

macro_rules! forward_to {
    ($baked:path => $($marker:ty),+ $(,)?) => {$(
        impl DataProvider<$marker> for RootCollation {
            fn load(&self, req: DataRequest) -> Result<DataResponse<$marker>, DataError> {
                $baked.load(req)
            }
        }
    )+};
}

forward_to!(icu_collator::provider::Baked =>
    CollationDiacriticsV1,
    CollationJamoV1,
    CollationMetadataV1,
    CollationReorderingV1,
    CollationRootV1,
    CollationSpecialPrimariesV1,
);
forward_to!(icu_normalizer::provider::Baked => NormalizerNfdDataV1, NormalizerNfdTablesV1);

impl DataProvider<CollationTailoringV1> for RootCollation {
    fn load(&self, req: DataRequest) -> Result<DataResponse<CollationTailoringV1>, DataError> {
        Err(DataErrorKind::IdentifierNotFound.with_req(CollationTailoringV1::INFO, req))
    }
}
