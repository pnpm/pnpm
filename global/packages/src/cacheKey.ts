import { createHexHash } from '@pnpm/crypto.hash'
import { lexCompare } from '@pnpm/util.lex-comparator'

export function createGlobalCacheKey (opts: {
  aliases: string[]
  registries: Record<string, string>
}): string {
  const sortedAliases = [...opts.aliases].sort(lexCompare)
  const sortedRegistries = Object.entries(opts.registries).sort(([k1], [k2]) => lexCompare(k1, k2))
  const hashStr = JSON.stringify([sortedAliases, sortedRegistries])
  return createHexHash(hashStr)
}
