import { type PassThrough } from 'stream'
import type { DeferredManifestPromise } from '@pnpm/cafs-types'
import concatStream from 'concat-stream'
import stripBom from 'strip-bom'

export function parseJsonBufferSync (buffer: Buffer) {
  return JSON.parse(stripBom(buffer.toString()))
}

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
  const _concatStream = concatStream((buffer) => {
    parseJsonBuffer(buffer, deferred)
  })
  stream.pipe(_concatStream)
  return () => {
    stream.unpipe(_concatStream)
  }
}
