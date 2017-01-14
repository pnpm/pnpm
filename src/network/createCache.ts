import cacheManager = require('cache-manager')
import fsStore = require('cache-manager-fs')
import thenify = require('thenify')
import logger from '../logger'

const cacheLogger = logger('cache')

export type CacheOptions = {
  path: string,
  ttl: number,
}

export default function createCache (opts: CacheOptions) {
  const cacheManagerOpts = {
    store: fsStore,
    ttl: opts.ttl,
    maxsize: 1000 * 1000 * 1000, // 1Gb, the max size in bytes on disk
    path: opts.path,
    preventfill: opts.ttl === 0,
  }
  const diskCache = cacheManager.caching(cacheManagerOpts)

  const getCache = thenify(diskCache.get.bind(diskCache))
  const setCache = thenify(diskCache.set.bind(diskCache))

  return {
    set: <T>(key: string, value: T) => setCache(key, value, cacheManagerOpts),
    get: async function (id: string) {
      try {
        return await getCache(id, cacheManagerOpts)
      } catch (err) {
        cacheLogger.error('cache', id)
        return null // ignore errors. The data can be requested.
      }
    },
  }
}