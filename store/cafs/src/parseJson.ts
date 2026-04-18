import stripBom from 'strip-bom'

import type { JsonParseCache } from './jsonCache.js'

export function parseJsonBufferSync (buffer: Buffer, cache?: JsonParseCache, digest?: string): unknown {
  if (cache && digest) {
    const cached = cache.get(digest)
    if (cached !== undefined) {
      return cached
    }
    const result = JSON.parse(stripBom(buffer.toString()))
    cache.set(digest, result)
    return result
  }
  return JSON.parse(stripBom(buffer.toString()))
}
