const CATALOG_PROTOCOL = 'catalog:'

/**
 * Parse a package.json dependency specifier using the catalog: protocol.
 * Returns null if the given specifier does not start with 'catalog:'.
 */
export function parseCatalogProtocol (bareSpecifier: string): string | 'default' | null {
  if (!bareSpecifier.startsWith(CATALOG_PROTOCOL)) {
    return null
  }

  const catalogNameRaw = bareSpecifier.slice(CATALOG_PROTOCOL.length).trim()

  // Allow a specifier of 'catalog:' to be a short-hand for 'catalog:default'.
  const catalogNameNormalized = catalogNameRaw === '' ? 'default' : catalogNameRaw

  return catalogNameNormalized
}
