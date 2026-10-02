import { type CafsFunctions, createCafs } from '@pnpm/store.cafs'
import type { Cafs } from '@pnpm/store.cafs-types'
import { createCafsStore } from '@pnpm/store.create-cafs-store'
import { ImmutableStoreIndex, StoreIndex } from '@pnpm/store.index'

import type { LinkPkgMessage } from './types.js'

const cafsCache = new Map<string, CafsFunctions>()
const cafsStoreCache = new Map<string, Cafs>()
const cafsLocker = new Map<string, number>()
const storeIndexCache = new Map<string, StoreIndex>()

export function getStoreIndex (storeDir: string, frozen = false): StoreIndex {
  // A frozen store is opened immutable (read-only), so it cannot share a
  // cached handle with a writable open of the same directory. Key on both.
  const cacheKey = frozen ? `${storeDir}\0frozen` : storeDir
  if (!storeIndexCache.has(cacheKey)) {
    storeIndexCache.set(cacheKey, frozen ? new ImmutableStoreIndex(storeDir) : new StoreIndex(storeDir))
  }
  return storeIndexCache.get(cacheKey)!
}

export function closeStoreIndexes (): void {
  for (const idx of storeIndexCache.values()) {
    idx.close()
  }
  storeIndexCache.clear()
}

export function getCafs (storeDir: string): CafsFunctions {
  if (!cafsCache.has(storeDir)) {
    cafsCache.set(storeDir, createCafs(storeDir))
  }
  return cafsCache.get(storeDir)!
}

export function getCafsStore ({ storeDir, packageImportMethod }: Pick<LinkPkgMessage, 'storeDir' | 'packageImportMethod'>): Cafs {
  const cacheKey = JSON.stringify({ storeDir, packageImportMethod })
  if (!cafsStoreCache.has(cacheKey)) {
    cafsStoreCache.set(cacheKey, createCafsStore(storeDir, { packageImportMethod, cafsLocker }))
  }
  return cafsStoreCache.get(cacheKey)!
}
