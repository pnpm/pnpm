import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { findDependencyLicenses } from '@pnpm/deps.compliance.license-scanner'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import type { DepPath, ProjectId, ProjectManifest } from '@pnpm/types'

const storeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-license-hoisted-'))
afterAll(() => {
  fs.rmSync(storeDir, { recursive: true, force: true })
})

function addToStore (name: string, integrity: string): void {
  const digest = `${name}0011223344`
  const dir = path.join(storeDir, 'files', digest.slice(0, 2))
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(path.join(dir, digest.slice(2)), JSON.stringify({ name, version: '1.0.0', license: 'MIT' }))
  const filesIndex: PackageFilesIndex = {
    algo: 'sha256',
    files: new Map([['package.json', { digest, mode: 0o644, size: 0 }]]),
  }
  const storeIndex = new StoreIndex(storeDir)
  storeIndex.set(storeIndexKey(integrity, `${name}@1.0.0`), filesIndex)
  storeIndex.close()
}

test('findDependencyLicenses() reports a peer variant the hoisted linker collapsed at the location it recorded', async () => {
  addToStore('alpha', 'sha512-alpha')
  addToStore('peer', 'sha512-peer')
  const lockfile: LockfileObject = {
    importers: {
      ['.' as ProjectId]: {
        dependencies: { alpha: '1.0.0(peer@1.0.0)', peer: '1.0.0' },
        specifiers: { alpha: '1.0.0', peer: '1.0.0' },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      ['alpha@1.0.0(peer@1.0.0)' as DepPath]: {
        dependencies: { peer: '1.0.0' },
        peerDependencies: { peer: '*' },
        resolution: { integrity: 'sha512-alpha' },
      },
      ['peer@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-peer' } },
    },
  }
  const lockfileDir = path.join(os.tmpdir(), 'project')

  const licenses = await findDependencyLicenses({
    lockfileDir,
    manifest: {} as ProjectManifest,
    virtualStoreDir: 'node_modules/.pnpm',
    virtualStoreDirMaxLength: 120,
    registriesByScope: { default: 'https://registry.npmjs.org/' },
    wantedLockfile: lockfile,
    storeDir,
    hoistedLocations: {
      'alpha@1.0.0(peer@2.0.0)': ['node_modules/alpha'],
      'peer@1.0.0': ['node_modules/peer'],
    },
  })

  expect(licenses.map(({ name, path }) => [name, path])).toStrictEqual([
    ['alpha', path.join(lockfileDir, 'node_modules', 'alpha')],
    ['peer', path.join(lockfileDir, 'node_modules', 'peer')],
  ])
})
