// Some packages still ship only the deprecated `licenses` array (e.g. busboy,
// streamsearch, limiter). See https://github.com/pnpm/pnpm/issues/11248.

export interface LicenseManifestEntry {
  type?: unknown
  name?: unknown
  url?: unknown
}

// Typed as `unknown` because real-world manifests use multiple shapes
// (string, object, array) and callers shouldn't need to cast.
export interface ManifestWithLicense {
  license?: unknown
  licenses?: unknown
}

/**
 * Derive a single license string from a package manifest.
 *
 * Prefers the modern `license` field, falling back to the deprecated `licenses`
 * array. Multiple entries are combined into an SPDX `OR` expression wrapped in
 * parentheses (e.g. `(MIT OR Apache-2.0)`). Returns `undefined` when no
 * license information can be derived.
 */
export function parseLicenseFromManifest (manifest: ManifestWithLicense): string | undefined {
  // `??` on the parse result (not the raw field) makes `license: ""` / `null` /
  // an object yielding no type defer to `licenses` instead of short-circuiting.
  return parseLicenseField(manifest.license) ?? parseLicenseField(manifest.licenses)
}

function parseLicenseField (field: unknown): string | undefined {
  if (typeof field === 'string') return field || undefined
  if (Array.isArray(field)) {
    const types = field
      .map(extractLicenseType)
      .filter((t): t is string => !!t)
    if (types.length === 0) return undefined
    if (types.length === 1) return types[0]
    return `(${types.join(' OR ')})`
  }
  if (field && typeof field === 'object') {
    return extractLicenseType(field)
  }
  return undefined
}

function extractLicenseType (entry: unknown): string | undefined {
  if (typeof entry === 'string') return entry || undefined
  if (!entry || typeof entry !== 'object') return undefined
  const { type, name } = entry as LicenseManifestEntry
  if (typeof type === 'string' && type) return type
  if (typeof name === 'string' && name) return name
  return undefined
}
