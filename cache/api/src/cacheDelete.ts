import { MetadataCache } from '@pnpm/cache.metadata'
import getRegistryName from 'encode-registry'

export async function cacheDelete (opts: { cacheDir: string, registry?: string }, filter: string[]): Promise<string> {
  const db = new MetadataCache(opts.cacheDir)
  try {
    const names = db.listNames()
    const prefix = opts.registry ? getRegistryName(opts.registry) : undefined
    const deleted: string[] = []
    for (const name of names) {
      const slashIdx = name.indexOf('/')
      if (slashIdx === -1) continue
      if (prefix && !name.startsWith(`${prefix}/`)) continue
      const pkgName = name.slice(slashIdx + 1)
      if (filter.length > 0 && !filter.some((f) => globMatch(pkgName, f))) continue
      db.delete(name)
      deleted.push(`${name}.json`)
    }
    return deleted.sort().join('\n')
  } finally {
    db.close()
  }
}

function globMatch (str: string, pattern: string): boolean {
  const regex = new RegExp(`^${pattern.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.')}$`)
  return regex.test(str)
}
