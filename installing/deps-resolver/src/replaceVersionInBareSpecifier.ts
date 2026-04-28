import semver from 'semver'

// `npm:` is the standard npm protocol; named-registry aliases (built-in `gh:`
// and any user-defined ones) follow the same `[<pkgName>@]<versionSelector>`
// body shape, so the same fast path applies. The set of named-registry
// prefixes is supplied by the caller — deps-resolver does not know which
// aliases are configured.
export function replaceVersionInBareSpecifier (
  bareSpecifier: string,
  version: string,
  namedRegistryPrefixes: readonly string[] = []
): string {
  if (semver.validRange(bareSpecifier)) {
    return version
  }
  const prefix = ['npm:', ...namedRegistryPrefixes].find((p) => bareSpecifier.startsWith(p))
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
