import { DeferredManifestPromise } from '@pnpm/fetcher-base'

export default function (
  buffer: Buffer,
  deferred: DeferredManifestPromise,
) {
  try {
    deferred.resolve(JSON.parse(buffer.toString()))
  } catch (err) {
    deferred.reject(err)
  }
}
