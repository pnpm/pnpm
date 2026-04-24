import semver from 'semver'

// Prefixes whose body has the shape `[<pkgName>@]<versionSelector>` and whose
// locked version can therefore be pasted in without touching any other resolver.
// `npm:` is the original case; `gh:` is pnpm's built-in named-registry alias.
// User-defined named-registry aliases aren't listed here — they skip the fast
// path (the specifier is returned unchanged), which means one extra metadata
// fetch on re-resolution but no correctness risk.
const REGISTRY_PREFIXES = ['npm:', 'gh:'] as const

// Replaces the range portion of a bare specifier with a concrete version so
// pnpm can skip the metadata fetch when the package is already locked to a
// specific id. Any specifier this function doesn't recognize is returned
// unchanged, letting other resolvers (git, tarball, local, workspace) keep
// their own semantics.
export function replaceVersionInBareSpecifier (bareSpecifier: string, version: string): string {
  if (semver.validRange(bareSpecifier)) {
    return version
  }
  const prefix = REGISTRY_PREFIXES.find((p) => bareSpecifier.startsWith(p))
  if (!prefix) return bareSpecifier
  const body = bareSpecifier.slice(prefix.length)

  // `<prefix>@scope/name[@range]` or `<prefix>pkg[@range]` — replace the range
  // after the last `@`. When the body starts with `@` and has no trailing
  // `@<range>`, `lastIndexOf` returns 0 (the scope marker), which `> 0`
  // correctly rejects so we don't treat the scope prefix as a version delimiter.
  const versionDelimiter = body.lastIndexOf('@')
  if (versionDelimiter > 0) {
    return `${prefix}${body.slice(0, versionDelimiter + 1)}${version}`
  }

  // `<prefix><range>` (e.g. `gh:^1.0.0` when paired with a scoped alias) —
  // replace the whole body with the version.
  if (!body.startsWith('@') && semver.validRange(body)) {
    return `${prefix}${version}`
  }

  // `<prefix><pkgName>` (no range) — append the version.
  return `${prefix}${body}@${version}`
}
