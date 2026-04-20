import { mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'

import { resolveLicenseFromDir } from '@pnpm/deps.compliance.license-resolver'

async function tempDir (): Promise<string> {
  return mkdtemp(path.join(tmpdir(), 'pnpm-resolve-license-from-dir-'))
}

describe('resolveLicenseFromDir', () => {
  test('returns the manifest license without touching disk', async () => {
    const dir = await tempDir()
    expect(
      await resolveLicenseFromDir({ manifest: { license: 'MIT' }, dir })
    ).toEqual({ name: 'MIT' })
  })

  test('scans the directory for a LICENSE file when the manifest has none', async () => {
    const dir = await tempDir()
    await writeFile(path.join(dir, 'LICENSE'), 'This project is released under the MIT license.', 'utf-8')

    const result = await resolveLicenseFromDir({ manifest: {}, dir })
    expect(result?.name).toBe('MIT')
    expect(result?.licenseFile).toContain('MIT')
  })

  test('resolves the legacy `licenses` array from the manifest', async () => {
    const dir = await tempDir()
    expect(
      await resolveLicenseFromDir({
        manifest: { licenses: [{ type: 'MIT' }] },
        dir,
      })
    ).toEqual({ name: 'MIT' })
  })

  test('prefers on-disk LICENSE over "SEE LICENSE IN" sentinel', async () => {
    const dir = await tempDir()
    await writeFile(path.join(dir, 'LICENSE.md'), 'Licensed under Apache-2.0', 'utf-8')

    expect(
      await resolveLicenseFromDir({
        manifest: { license: 'SEE LICENSE IN LICENSE.md' },
        dir,
      })
    ).toMatchObject({ name: 'Apache-2.0' })
  })

  test('returns undefined when the directory has no manifest license and no LICENSE file', async () => {
    const dir = await tempDir()
    expect(
      await resolveLicenseFromDir({ manifest: {}, dir })
    ).toBeUndefined()
  })
})
