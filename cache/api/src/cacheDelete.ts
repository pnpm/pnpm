import { MetadataCache } from '@pnpm/cache.metadata'

export async function cacheDelete (opts: { cacheDir: string, registry?: string }, filter: string[]): Promise<string> {
  const db = new MetadataCache(opts.cacheDir)
  try {
    const names = db.listNames()
    const prefix = opts.registry ? new URL(opts.registry).host : undefined
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
