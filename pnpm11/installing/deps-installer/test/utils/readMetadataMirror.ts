import fs from 'node:fs'
import path from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'
import util from 'node:util'

import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import type { PackageManifest } from '@pnpm/types'

export interface MetadataMirror {
  raw: string
  meta: { versions: Record<string, PackageManifest> }
}

/**
 * Reads a package's metadata mirror for the mocked registry. The resolver
 * does not await the mirror write, so an install can return before the file
 * is in place.
 */
export async function readMetadataMirror (cacheDir: string, pkgName: string): Promise<MetadataMirror> {
  const mirrorPath = path.join(cacheDir, ABBREVIATED_META_DIR, `http%3A+localhost+${REGISTRY_MOCK_PORT}`, `${pkgName}.jsonl`)
  let lastError: unknown
  /* eslint-disable no-await-in-loop */
  for (let attempt = 0; attempt < 20; attempt++) {
    let raw: string
    try {
      raw = await fs.promises.readFile(mirrorPath, 'utf8')
    } catch (err: unknown) {
      if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') throw err
      lastError = err
      await delay(100)
      continue
    }
    return { raw, meta: JSON.parse(raw.slice(raw.indexOf('\n') + 1)) }
  }
  /* eslint-enable no-await-in-loop */
  throw lastError
}
