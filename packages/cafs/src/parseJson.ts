import { PassThrough } from 'stream'
import { DeferredManifestPromise } from '@pnpm/fetcher-base'
import concatStream from 'concat-stream'
import stripBom from 'strip-bom'

export function parseJsonBuffer (
  buffer: Buffer,
  deferred: DeferredManifestPromise
) {
  try {
    deferred.resolve(JSON.parse(stripBom(buffer.toString())))
  } catch (err: any) { // eslint-disable-line
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
