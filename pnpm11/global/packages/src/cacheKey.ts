import { createHexHash } from '@pnpm/crypto.hash'
import { lexCompare } from '@pnpm/text.ordinal-comparator'

export function createGlobalCacheKey (opts: {
  aliases: string[]
  registriesByScope: Record<string, string>
}): string {
  const sortedAliases = [...opts.aliases].sort(lexCompare)
  const sortedRegistries = Object.entries(opts.registriesByScope).sort(([k1], [k2]) => lexCompare(k1, k2))
  const hashStr = JSON.stringify([sortedAliases, sortedRegistries])
  return createHexHash(hashStr)
}
