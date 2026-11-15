use super::{Config, FilterSource};

impl Config {
    /// [`Self::filter`] and [`Self::filter_prod`] paired with their source in
    /// evaluation order; see [`ordered_filter_entries`].
    #[must_use]
    pub fn ordered_filters(&self) -> Vec<(FilterSource, &str)> {
        ordered_filter_entries(&self.filter, &self.filter_prod, &self.filter_order)
    }
}

/// Pair `filter` and `filter_prod` selectors with their [`FilterSource`] in
/// evaluation order.
///
/// When `filter_order` records an exact interleaving for `(filter, filter_prod)`,
/// that command-line order is preserved. Otherwise `filter` precedes
/// `filter_prod`, except when `filter` begins with an exclusion (`!`) while
/// `filter_prod` begins with an inclusion, where placing `filter_prod` first
/// keeps the inclusion as the base set rather than seeding every workspace
/// project from the leading exclusion.
#[must_use]
pub fn ordered_filter_entries<'a>(
    filter: &'a [String],
    filter_prod: &'a [String],
    filter_order: &[FilterSource],
) -> Vec<(FilterSource, &'a str)> {
    if filter.is_empty() {
        return filter_prod
            .iter()
            .map(|selector| (FilterSource::FilterProd, selector.as_str()))
            .collect();
    }
    if filter_prod.is_empty() {
        return filter
            .iter()
            .map(|selector| (FilterSource::Filter, selector.as_str()))
            .collect();
    }
    if let Some(interleaved) = interleave_filters(filter, filter_prod, filter_order) {
        return interleaved;
    }
    let prod_first = filter[0].starts_with('!') && !filter_prod[0].starts_with('!');
    let regular = filter
        .iter()
        .map(|selector| (FilterSource::Filter, selector.as_str()));
    let prod = filter_prod
        .iter()
        .map(|selector| (FilterSource::FilterProd, selector.as_str()));
    if prod_first { prod.chain(regular).collect() } else { regular.chain(prod).collect() }
}

fn interleave_filters<'a>(
    filter: &'a [String],
    filter_prod: &'a [String],
    filter_order: &[FilterSource],
) -> Option<Vec<(FilterSource, &'a str)>> {
    if filter_order.len() != filter.len() + filter_prod.len() {
        return None;
    }
    let mut regular_iter = filter.iter();
    let mut prod_iter = filter_prod.iter();
    let mut entries = Vec::with_capacity(filter_order.len());
    for &source in filter_order {
        let selector = match source {
            FilterSource::Filter => regular_iter.next()?,
            FilterSource::FilterProd => prod_iter.next()?,
        };
        entries.push((source, selector.as_str()));
    }
    (regular_iter.next().is_none() && prod_iter.next().is_none()).then_some(entries)
}
