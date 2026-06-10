import { createHash } from 'node:crypto'
import { gunzipSync } from 'node:zlib'

import { PnpmError } from '@pnpm/error'
import tar from 'tar-stream'

import { extractBundledDependencies, type PublishSummary } from './publishSummary.js'
import { createTarballFilename } from './safeTarballFilename.js'

interface TarballManifest {
  _id?: string
  name?: string
  version?: string
  bundledDependencies?: unknown
  bundleDependencies?: unknown
  dependencies?: Record<string, unknown>
}

/**
 * Parse a packed (gzipped or plain) tarball buffer and return the same
 * {@link PublishSummary} shape that `pnpm publish --json` emits.
 *
 * Used when we hold the tarball bytes already and need a summary without
 * re-packing — e.g. inspecting a staged publish via `pnpm stage download`.
 *
 * @throws {@link PnpmError} with code `STAGE_TARBALL_MANIFEST_NOT_FOUND` when the tarball
 *   does not contain `package/package.json`, or when that file is unparseable JSON.
 */
export async function summarizeTarball (tarballData: Buffer): Promise<PublishSummary> {
  const extract = tar.extract()
  const files: Array<{ path: string }> = []
  const bundled = new Set<string>()
  let manifest: TarballManifest | undefined
  let entryCount = 0
  let unpackedSize = 0

  await new Promise<void>((resolve, reject) => {
    extract.on('entry', (header, stream, next) => {
      const chunks: Buffer[] = []
      if (header.type === 'file') {
        entryCount++
        unpackedSize += header.size ?? 0
        files.push({ path: header.name.replace(/^package\//, '') })
        const bundledMatch = /^package\/node_modules\/((?:@[^/]+\/)?[^/]+)/.exec(header.name)
        if (bundledMatch?.[1]) {
          bundled.add(bundledMatch[1])
        }
      }
      if (header.name === 'package/package.json') {
        stream.on('data', (chunk: Buffer) => chunks.push(Buffer.from(chunk)))
      }
      stream.on('error', reject)
      stream.on('end', () => {
        if (header.name === 'package/package.json') {
          try {
            manifest = JSON.parse(Buffer.concat(chunks).toString())
          } catch (error: unknown) {
            reject(error)
            return
          }
        }
        next()
      })
      stream.resume()
    })
    extract.on('error', reject)
    extract.on('finish', resolve)
    extract.end(maybeGunzip(tarballData))
  })

  if (!manifest?.name || !manifest.version) {
    throw new PnpmError('STAGE_TARBALL_MANIFEST_NOT_FOUND', 'Could not read package.json from tarball')
  }

  files.sort((a, b) => a.path.localeCompare(b.path, 'en'))
  return {
    id: manifest._id ?? `${manifest.name}@${manifest.version}`,
    name: manifest.name,
    version: manifest.version,
    size: tarballData.byteLength,
    unpackedSize,
    shasum: createHash('sha1').update(tarballData).digest('hex'),
    integrity: `sha512-${createHash('sha512').update(tarballData).digest('base64')}`,
    filename: createTarballFilename({ name: manifest.name, version: manifest.version }),
    files,
    entryCount,
    bundled: bundled.size > 0 ? Array.from(bundled).sort() : extractBundledDependencies(manifest),
  }
}

function maybeGunzip (tarballData: Buffer): Buffer {
  try {
    return gunzipSync(tarballData)
  } catch {
    return tarballData
  }
}
