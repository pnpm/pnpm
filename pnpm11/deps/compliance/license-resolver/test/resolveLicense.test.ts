import { mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { resolveLicense } from '@pnpm/deps.compliance.license-resolver'

async function tempDir (): Promise<string> {
  return mkdtemp(path.join(tmpdir(), 'pnpm-resolve-license-'))
}

async function writeLicenseFile (dir: string, name: string, content: string): Promise<string> {
  const p = path.join(dir, name)
  await writeFile(p, content, 'utf-8')
  return p
}

describe('resolveLicense', () => {
  test('returns the manifest license when present', async () => {
    expect(
      await resolveLicense({ manifest: { license: 'MIT' }, files: new Map() })
    ).toEqual({ name: 'MIT' })
  })

  test('falls back to the legacy `licenses` array', async () => {
    expect(
      await resolveLicense({
        manifest: { licenses: [{ type: 'MIT' }] },
        files: new Map(),
      })
    ).toEqual({ name: 'MIT' })
  })

  test('scans LICENSE file when manifest has no license', async () => {
    const dir = await tempDir()
    const licensePath = await writeLicenseFile(dir, 'LICENSE', 'This software is released under the MIT license.')
    const files = new Map([['LICENSE', licensePath]])

    expect(
      await resolveLicense({ manifest: {}, files })
    ).toEqual({ name: 'MIT', licenseFile: expect.stringContaining('MIT') })
  })

  test('prefers LICENSE file over "SEE LICENSE IN" sentinel', async () => {
    const dir = await tempDir()
    const licensePath = await writeLicenseFile(dir, 'LICENSE.md', 'Licensed under Apache-2.0')
    const files = new Map([['LICENSE.md', licensePath]])

    expect(
      await resolveLicense({
        manifest: { license: 'SEE LICENSE IN LICENSE.md' },
        files,
      })
    ).toMatchObject({ name: 'Apache-2.0' })
  })

  test('returns "Unknown" when LICENSE file exists but has no recognizable text', async () => {
    const dir = await tempDir()
    const licensePath = await writeLicenseFile(dir, 'LICENSE', 'Completely custom proprietary terms follow...')
    const files = new Map([['LICENSE', licensePath]])

    const result = await resolveLicense({ manifest: {}, files })
    expect(result?.name).toBe('Unknown')
    expect(result?.licenseFile).toContain('Completely custom')
  })

  test('returns undefined when neither manifest nor LICENSE file exist', async () => {
    expect(
      await resolveLicense({ manifest: {}, files: new Map() })
    ).toBeUndefined()
  })

  test('returns the "SEE LICENSE IN" sentinel string when no LICENSE file exists', async () => {
    expect(
      await resolveLicense({
        manifest: { license: 'SEE LICENSE IN LICENSE.md' },
        files: new Map(),
      })
    ).toEqual({ name: 'SEE LICENSE IN LICENSE.md' })
  })

  test('joins multiple detected license names with OR', async () => {
    const dir = await tempDir()
    const licensePath = await writeLicenseFile(dir, 'LICENSE', 'Dual-licensed under MIT and Apache-2.0')
    const files = new Map([['LICENSE', licensePath]])

    const result = await resolveLicense({ manifest: {}, files })
    // Both names are detected; de-duplicated, joined with OR.
    expect(result?.name.split(' OR ').sort()).toEqual(['Apache-2.0', 'MIT'])
  })

  // Precedence end-to-end — see #11248.
  test('modern `license` wins over both legacy `licenses` and on-disk LICENSE', async () => {
    const dir = await tempDir()
    const licensePath = await writeLicenseFile(dir, 'LICENSE', 'Licensed under ISC')
    const files = new Map([['LICENSE', licensePath]])

    expect(
      await resolveLicense({
        manifest: {
          license: 'Apache-2.0',
          licenses: [{ type: 'MIT' }],
        },
        files,
      })
    ).toEqual({ name: 'Apache-2.0' })
  })

  test('on-disk LICENSE is ignored when the manifest has a real SPDX id', async () => {
    const dir = await tempDir()
    const licensePath = await writeLicenseFile(dir, 'LICENSE', 'Licensed under ISC')
    const files = new Map([['LICENSE', licensePath]])

    expect(
      await resolveLicense({ manifest: { license: 'MIT' }, files })
    ).toEqual({ name: 'MIT' })
  })
})
