import { createHexHash } from '@pnpm/crypto.hash'
import { type SupportedArchitectures } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'

export function createCacheKey (opts: {
  packages: string[]
  registries: Record<string, string>
  allowBuild?: string[]
  supportedArchitectures?: SupportedArchitectures
}): string {
  const sortedPkgs = [...opts.packages].sort((a, b) => a.localeCompare(b))
  const sortedRegistries = Object.entries(opts.registries).sort(([k1], [k2]) => k1.localeCompare(k2))
  const args: unknown[] = [sortedPkgs, sortedRegistries]
  if (opts.allowBuild?.length) {
    args.push({ allowBuild: opts.allowBuild.sort(lexCompare) })
  }
  if (opts.supportedArchitectures) {
    const supportedArchitecturesKeys = ['cpu', 'libc', 'os'] as const satisfies Array<keyof SupportedArchitectures>
    for (const key of supportedArchitecturesKeys) {
      const value = opts.supportedArchitectures[key]
      if (!value?.length) continue
      args.push({
        supportedArchitectures: {
          [key]: [...new Set(value)].sort(lexCompare),
        },
      })
    }
  }
  const hashStr = JSON.stringify(args)
  return createHexHash(hashStr)
}

export function createGlobalCacheKey (opts: {
  aliases: string[]
  registries: Record<string, string>
}): string {
  const sortedAliases = [...opts.aliases].sort((a, b) => a.localeCompare(b))
  const sortedRegistries = Object.entries(opts.registries).sort(([k1], [k2]) => k1.localeCompare(k2))
  const hashStr = JSON.stringify([sortedAliases, sortedRegistries])
  return createHexHash(hashStr)
}
