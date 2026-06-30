/// <reference path="../../../__typings__/local.d.ts"/>
import { expect, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.pruner'
import type { DepPath, ProjectId, Registries } from '@pnpm/types'

import type { DependenciesGraph } from '../lib/index.js'
import { updateLockfile } from '../lib/updateLockfile.js'

const TARBALL_URL = 'https://cdn.sheetjs.com/xlsx-0.18.5/xlsx-0.18.5.tgz'
const DEP_PATH = `xlsx@${TARBALL_URL}` as DepPath
const INTEGRITY = 'sha512-AaaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaAaA=='
const REGISTRIES: Registries = { default: 'https://registry.npmjs.org/' }

function tarballGraph (resolution: { tarball: string, integrity?: string }): DependenciesGraph {
  return {
    [DEP_PATH]: {
      name: 'xlsx',
      version: '0.18.5',
      resolution,
      optional: false,
      children: {},
      optionalDependencies: new Set<string>(),
      peerDependencies: {},
      transitivePeerDependencies: new Set<string>(),
      additionalInfo: {},
      hasBin: false,
    },
  } as unknown as DependenciesGraph
}

function lockfileWith (snapshot: PackageSnapshot): LockfileObject {
  return {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        dependencies: { xlsx: TARBALL_URL },
        specifiers: { xlsx: TARBALL_URL },
      },
    },
    packages: { [DEP_PATH]: snapshot },
  }
}

test('integrity of a remote tarball dependency is carried over when its lockfile entry is rebuilt without an integrity', () => {
  const lockfile = updateLockfile({
    dependenciesGraph: tarballGraph({ tarball: TARBALL_URL }),
    lockfile: lockfileWith({ resolution: { tarball: TARBALL_URL, integrity: INTEGRITY } }),
    prefix: '.',
    registries: REGISTRIES,
  })
  expect(lockfile.packages![DEP_PATH].resolution).toStrictEqual({ tarball: TARBALL_URL, integrity: INTEGRITY })
})

test('a freshly resolved integrity is never overwritten by the previous one', () => {
  const newIntegrity = 'sha512-BbbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbBbB=='
  const lockfile = updateLockfile({
    dependenciesGraph: tarballGraph({ tarball: TARBALL_URL, integrity: newIntegrity }),
    lockfile: lockfileWith({ resolution: { tarball: TARBALL_URL, integrity: INTEGRITY } }),
    prefix: '.',
    registries: REGISTRIES,
  })
  expect(lockfile.packages![DEP_PATH].resolution).toStrictEqual({ tarball: TARBALL_URL, integrity: newIntegrity })
})

test('a stale integrity is not attached when the tarball URL changed', () => {
  const newUrl = 'https://cdn.sheetjs.com/xlsx-0.19.0/xlsx-0.19.0.tgz'
  const newDepPath = `xlsx@${newUrl}` as DepPath
  const lockfile = updateLockfile({
    dependenciesGraph: {
      [newDepPath]: tarballGraph({ tarball: newUrl })[DEP_PATH],
    } as unknown as DependenciesGraph,
    lockfile: {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {
        ['.' as ProjectId]: {
          dependencies: { xlsx: newUrl },
          specifiers: { xlsx: newUrl },
        },
      },
      packages: { [DEP_PATH]: { resolution: { tarball: TARBALL_URL, integrity: INTEGRITY } } },
    },
    prefix: '.',
    registries: REGISTRIES,
  })
  expect(lockfile.packages![newDepPath].resolution).toStrictEqual({ tarball: newUrl })
})
