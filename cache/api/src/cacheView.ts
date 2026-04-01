import { MetadataCache } from '@pnpm/cache.metadata'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'

interface CachedVersions {
  cachedVersions: string[]
  nonCachedVersions: string[]
  cachedAt?: string
  distTags: Record<string, string>
}

export async function cacheView (opts: { cacheDir: string, storeDir: string, registry?: string }, packageName: string): Promise<string> {
  const db = new MetadataCache(opts.cacheDir)
  const storeIndex = new StoreIndex(opts.storeDir)
  try {
    const names = db.listNames()
    const prefix = opts.registry ? new URL(opts.registry).host : undefined
    const result: Record<string, CachedVersions> = {}
    for (const name of names) {
      const slashIdx = name.indexOf('/')
      if (slashIdx === -1) continue
      if (prefix && !name.startsWith(`${prefix}/`)) continue
      const pkgName = name.slice(slashIdx + 1)
      if (pkgName !== packageName) continue
      const registryName = name.slice(0, slashIdx)
      const index = db.getIndex(name)
      if (!index) continue
      const distTags = JSON.parse(index.distTags) as Record<string, string>
      const versionsMap = JSON.parse(index.versions) as Record<string, unknown>
      const cachedVersions: string[] = []
      const nonCachedVersions: string[] = []
      for (const version of Object.keys(versionsMap)) {
        // Load manifest to check integrity
        const manifestJson = db.getManifest(name, version, 'abbreviated')
        if (!manifestJson) {
          nonCachedVersions.push(version)
          continue
        }
        const manifest = JSON.parse(manifestJson) as { name?: string, dist?: { integrity?: string } }
        if (!manifest.dist?.integrity) {
          nonCachedVersions.push(version)
          continue
        }
        const key = storeIndexKey(manifest.dist.integrity, `${manifest.name ?? pkgName}@${version}`)
        if (storeIndex.has(key)) {
          cachedVersions.push(version)
        } else {
          nonCachedVersions.push(version)
        }
      }
      result[registryName] = {
        cachedVersions,
        nonCachedVersions,
        cachedAt: index.cachedAt ? new Date(index.cachedAt).toString() : undefined,
        distTags,
      }
    }
    return JSON.stringify(result, null, 2)
  } finally {
    storeIndex.close()
    db.close()
  }
}
