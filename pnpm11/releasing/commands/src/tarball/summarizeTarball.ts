import { createHash } from 'node:crypto'
import util from 'node:util'
import { gunzipSync } from 'node:zlib'

import { PnpmError } from '@pnpm/error'
import stripBom from 'strip-bom'
import tar from 'tar-stream'

import { extractBundledDependencies, type PublishSummary } from './publishSummary.js'
import { createTarballFilename, validatePackageIdentity } from './safeTarballFilename.js'

/** Maximum compressed or decompressed staged tarball size. */
export const MAX_TARBALL_BYTES = 512 * 1024 * 1024

export interface TarballManifest {
  _id?: string
  name: string
  version: string
  bundledDependencies?: unknown
  bundleDependencies?: unknown
  dependencies?: Record<string, unknown>
  devDependencies?: Record<string, unknown>
  optionalDependencies?: Record<string, unknown>
  peerDependencies?: Record<string, unknown>
}

interface TarballContents {
  bundled: Set<string>
  entryCount: number
  files: Array<{ path: string }>
  manifest: TarballManifest
  unpackedSize: number
}

/**
 * Parse a packed (gzipped or plain) tarball buffer and return the same
 * {@link PublishSummary} shape that `pnpm publish --json` emits.
 *
 * Used when we hold the tarball bytes already and need a summary without
 * re-packing — e.g. inspecting a staged publish via `pnpm stage download`.
 *
 * @throws {@link PnpmError} with code `STAGE_TARBALL_MANIFEST_NOT_FOUND` when the tarball
 *   does not contain `package/package.json`, or when that file is unparsable JSON.
 */
export async function summarizeTarball (tarballData: Buffer): Promise<PublishSummary> {
  const { bundled, entryCount, files, manifest, unpackedSize } = await readTarballContents(tarballData, true)

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

/** Read the published package.json held in a packed tarball. */
export async function readTarballManifest (tarballData: Buffer): Promise<TarballManifest> {
  return (await readTarballContents(tarballData, false)).manifest
}

async function readTarballContents (tarballData: Buffer, includeSummary: boolean): Promise<TarballContents> {
  const extract = tar.extract()
  const files: Array<{ path: string }> = []
  const bundled = new Set<string>()
  let manifestText: Buffer | undefined
  let entryCount = 0
  let unpackedSize = 0

  await new Promise<void>((resolve, reject) => {
    extract.on('entry', (header, stream, next) => {
      const chunks: Buffer[] = []
      if (includeSummary && header.type === 'file') {
        entryCount++
        unpackedSize += header.size ?? 0
        files.push({ path: header.name.replace(/^package\//, '') })
        const bundledMatch = /^package\/node_modules\/((?:@[^/]+\/)?[^/]+)/.exec(header.name)
        if (bundledMatch?.[1]) {
          bundled.add(bundledMatch[1])
        }
      }
      if (header.name === 'package/package.json') {
        stream.on('data', (chunk) => chunks.push(Buffer.from(chunk as Uint8Array)))
      }
      stream.on('error', reject)
      stream.on('end', () => {
        if (header.name === 'package/package.json') {
          manifestText = Buffer.concat(chunks)
        }
        next()
      })
      stream.resume()
    })
    extract.on('error', reject)
    extract.on('finish', resolve)
    extract.end(maybeGunzip(tarballData))
  })

  let parsedManifest: unknown
  try {
    parsedManifest = JSON.parse(stripBom(manifestText?.toString() ?? ''))
  } catch {
    throw new PnpmError('STAGE_TARBALL_MANIFEST_NOT_FOUND', 'Could not read package.json from tarball')
  }
  if (parsedManifest == null || typeof parsedManifest !== 'object' || Array.isArray(parsedManifest)) {
    throw new PnpmError('STAGE_TARBALL_MANIFEST_NOT_FOUND', 'Could not read package.json from tarball')
  }
  const manifest = parsedManifest as Partial<TarballManifest>
  if (typeof manifest.name !== 'string' || manifest.name.length === 0 ||
      typeof manifest.version !== 'string' || manifest.version.length === 0) {
    throw new PnpmError('STAGE_TARBALL_MANIFEST_NOT_FOUND', 'Could not read package.json from tarball')
  }
  validatePackageIdentity({ name: manifest.name, version: manifest.version })

  return {
    bundled,
    entryCount,
    files,
    manifest: manifest as TarballManifest,
    unpackedSize,
  }
}

function maybeGunzip (tarballData: Buffer): Buffer {
  try {
    return gunzipSync(tarballData, { maxOutputLength: MAX_TARBALL_BYTES })
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ERR_BUFFER_TOO_LARGE') {
      throw new PnpmError(
        'STAGE_REGISTRY_ERROR',
        `Failed to read the staged tarball: tarball exceeded ${MAX_TARBALL_BYTES} bytes when decompressed`
      )
    }
    return tarballData
  }
}
