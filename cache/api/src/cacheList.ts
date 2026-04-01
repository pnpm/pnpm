import { MetadataCache } from '@pnpm/cache.metadata'

export async function cacheListRegistries (opts: { cacheDir: string }): Promise<string> {
  const db = new MetadataCache(opts.cacheDir)
  try {
    const names = db.listNames()
    const registries = new Set<string>()
    for (const name of names) {
      const slashIdx = name.indexOf('/')
      if (slashIdx !== -1) {
        registries.add(name.slice(0, slashIdx))
      }
    }
    return [...registries].sort().join('\n')
  } finally {
    db.close()
  }
}

export async function cacheList (opts: { cacheDir: string, registry?: string }, filter: string[]): Promise<string> {
  const db = new MetadataCache(opts.cacheDir)
  try {
    const names = db.listNames()
    const prefix = opts.registry ? new URL(opts.registry).host : undefined
    const results: string[] = []
    for (const name of names) {
      const slashIdx = name.indexOf('/')
      if (slashIdx === -1) continue
      if (prefix && !name.startsWith(`${prefix}/`)) continue
      const pkgName = name.slice(slashIdx + 1)
      if (filter.length > 0 && !filter.some((f) => globMatch(pkgName, f))) continue
      results.push(`${name}.json`)
    }
    return results.sort().join('\n')
  } finally {
    db.close()
  }
}

function globMatch (str: string, pattern: string): boolean {
  const regex = new RegExp(`^${pattern.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.')}$`)
  return regex.test(str)
}
