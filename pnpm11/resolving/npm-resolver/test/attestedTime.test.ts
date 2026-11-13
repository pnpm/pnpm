import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { ATTESTED_TIME_DIR } from '@pnpm/constants'
import { temporaryDirectory } from 'tempy'

import { appendAttestedTime, loadAttestedTimes } from '../src/attestedTime.js'
import { getPkgMirrorPath } from '../src/pickPackage.js'

const registry = 'https://registry.npmjs.org/'

test('appended attested times are loaded by version', async () => {
  const location = { cacheDir: temporaryDirectory(), registry, name: '@scope/pkg' }

  await appendAttestedTime(location, { version: '1.0.0', publishedAt: '2025-01-01T00:00:00.000Z' })
  await appendAttestedTime(location, { version: '1.1.0', publishedAt: '2025-02-01T00:00:00.000Z' })

  expect(await loadAttestedTimes(location)).toStrictEqual({
    '1.0.0': '2025-01-01T00:00:00.000Z',
    '1.1.0': '2025-02-01T00:00:00.000Z',
  })
})

test('a line that does not parse is skipped', async () => {
  const location = { cacheDir: temporaryDirectory(), registry, name: 'pkg' }
  const file = getPkgMirrorPath(location.cacheDir, ATTESTED_TIME_DIR, registry, 'pkg')
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.writeFileSync(file, '{"version":"1.0.0","publ\n')

  await appendAttestedTime(location, { version: '1.1.0', publishedAt: '2025-02-01T00:00:00.000Z' })

  expect(await loadAttestedTimes(location)).toStrictEqual({ '1.1.0': '2025-02-01T00:00:00.000Z' })
})

test('a missing file holds no attested times', async () => {
  expect(await loadAttestedTimes({ cacheDir: temporaryDirectory(), registry, name: 'pkg' })).toStrictEqual({})
})
