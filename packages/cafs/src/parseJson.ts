import { DeferredManifestPromise } from '@pnpm/fetcher-base'
import concatStream = require('concat-stream')
import { PassThrough } from 'stream'

export function parseJsonBuffer (
  buffer: Buffer,
  deferred: DeferredManifestPromise
) {
  try {
    deferred.resolve(JSON.parse(buffer.toString()))
  } catch (err) {
    deferred.reject(err)
  }
}

export function parseJsonStream (
  stream: PassThrough,
  deferred: DeferredManifestPromise
) {
  stream.pipe(
    concatStream((buffer) => parseJsonBuffer(buffer, deferred))
  )
}
