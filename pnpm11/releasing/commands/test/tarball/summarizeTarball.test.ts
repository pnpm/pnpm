import { describe, expect, test } from '@jest/globals'
import tar from 'tar-stream'

import { readTarballManifest } from '../../src/tarball/summarizeTarball.js'

describe('readTarballManifest', () => {
  test('uses the final package.json entry when a tarball contains duplicates', async () => {
    const tarball = await createTarball([
      '{',
      JSON.stringify({ name: 'pkg', version: '2.0.0' }),
    ])

    await expect(readTarballManifest(tarball)).resolves.toMatchObject({
      name: 'pkg',
      version: '2.0.0',
    })
  })

  test('rejects an invalid package identity', async () => {
    const invalidName = await createTarball([
      JSON.stringify({ name: '__proto__', version: '1.0.0' }),
    ])
    const invalidVersion = await createTarball([
      JSON.stringify({ name: 'pkg', version: 'not a version' }),
    ])

    await expect(readTarballManifest(invalidName)).rejects.toMatchObject({
      code: 'ERR_PNPM_INVALID_PACKAGE_NAME',
    })
    await expect(readTarballManifest(invalidVersion)).rejects.toMatchObject({
      code: 'ERR_PNPM_INVALID_PACKAGE_VERSION',
    })
  })

  test('accepts a UTF-8 BOM before the manifest', async () => {
    const tarball = await createTarball([
      `\uFEFF${JSON.stringify({ name: 'pkg', version: '1.0.0' })}`,
    ])

    await expect(readTarballManifest(tarball)).resolves.toMatchObject({
      name: 'pkg',
      version: '1.0.0',
    })
  })
})

async function createTarball (manifests: string[]): Promise<Buffer> {
  const pack = tar.pack()
  const chunks: Buffer[] = []
  pack.on('data', (chunk) => chunks.push(Buffer.from(chunk as Uint8Array)))
  const completed = new Promise<Buffer>((resolve, reject) => {
    pack.on('error', reject)
    pack.on('end', () => resolve(Buffer.concat(chunks)))
    addManifest(0)

    function addManifest (index: number): void {
      const manifest = manifests[index]
      if (manifest == null) {
        pack.finalize()
        return
      }
      pack.entry({ name: 'package/package.json' }, manifest, (error?: Error | null) => {
        if (error) reject(error)
        else addManifest(index + 1)
      })
    }
  })
  return completed
}
