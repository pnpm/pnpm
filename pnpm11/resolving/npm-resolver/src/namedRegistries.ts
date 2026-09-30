import { isWellFormedRegistryName, RESERVED_VERSION_PREFIXES } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'

import { BUILTIN_REGISTRIES_BY_PREFIX } from './parseBareSpecifier.js'

// Merges user-supplied named-registry aliases (from config) on top of pnpm's
// built-in defaults (e.g. `gh` → GitHub Packages). User entries take precedence
// so GHES users can point `gh` at their enterprise host. URLs are validated
// here so typos like `npm.work.example.com` (no scheme) surface at startup
// rather than as a confusing 404 during resolution. The named-registry
// resolver runs last in the resolution chain, so an alias that collides with
// another specifier scheme (e.g. `git`, `github`, `jsr`) is silently shadowed
// by that scheme's dedicated resolver — no cross-resolver knowledge needed.
export function mergeNamedRegistries (userDefined?: Record<string, string>): Record<string, string> {
  const merged: Record<string, string> = { ...BUILTIN_REGISTRIES_BY_PREFIX }
  if (!userDefined) return merged
  for (const [alias, url] of Object.entries(userDefined)) {
    validateNamedRegistryAlias(alias)
    validateNamedRegistryUrl(alias, url)
    merged[alias] = url
  }
  return merged
}

function validateNamedRegistryAlias (alias: string): void {
  const isReserved = RESERVED_VERSION_PREFIXES.has(alias)
  if (!isReserved && isWellFormedRegistryName(alias)) return
  throw new PnpmError(
    'RESERVED_NAMED_REGISTRY_NAME',
    isReserved
      ? `'${alias}' cannot be used as a named registry alias: it is a reserved dependency specifier prefix.`
      : `'${alias}' cannot be used as a named registry alias: aliases must start with a letter and contain only letters, digits, ".", "_", and "-".`,
    { hint: 'Change the prefix on the corresponding registries entry.' }
  )
}

function validateNamedRegistryUrl (alias: string, url: unknown): void {
  if (typeof url === 'string' && isValidHttpUrl(url)) return
  throw new PnpmError(
    'INVALID_NAMED_REGISTRY_URL',
    `The named registry alias '${alias}' is mapped to '${String(url)}', which is not a valid http(s) URL.`,
    { hint: 'Provide a URL that starts with http:// or https://, e.g. https://npm.pkg.example.com/' }
  )
}

function isValidHttpUrl (url: string): boolean {
  try {
    const parsed = new URL(url)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}
