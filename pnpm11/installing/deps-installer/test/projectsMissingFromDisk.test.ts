import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.types'

import { listProjectsMissingFromDisk } from '../lib/install/projectsMissingFromDisk.js'

const lockfileDir = path.resolve('workspace')

function standInManifests (importers: Record<string, ProjectSnapshot>) {
  return listProjectsMissingFromDisk({
    lockfileDir,
    projects: {},
    wantedLockfile: { importers: importers as LockfileObject['importers'], lockfileVersion: '9.0' },
  }).map(({ id, manifest }) => ({ id, manifest }))
}

test('a stand-in declares the registry dependencies its importer records', () => {
  expect(standInManifests({
    'project-3': {
      specifiers: {
        'is-positive': '^1.0.0',
        'is-negative': 'npm:is-negative@^1.0.0',
        tagged: 'repo.tgz',
        'local-pkg': 'file:../local-pkg',
        'local-alias': 'file:../local-pkg',
        'project-2': 'workspace:*',
      },
      dependencies: {
        'is-positive': '1.0.0',
        'is-negative': 'is-negative@1.0.0',
        tagged: '1.0.0',
        'local-pkg': 'file:local-pkg',
        'local-alias': 'local-pkg@file:local-pkg',
        'project-2': 'link:../project-2',
      },
    },
  })).toStrictEqual([{
    id: 'project-3',
    manifest: {
      dependencies: {
        'is-positive': '^1.0.0',
        'is-negative': 'npm:is-negative@^1.0.0',
        tagged: 'repo.tgz',
      },
    },
  }])
})

test('an importer outside the workspace gets no stand-in', () => {
  expect(standInManifests({
    '../outside': {
      specifiers: { 'is-positive': '^1.0.0' },
      dependencies: { 'is-positive': '1.0.0' },
    },
  })).toStrictEqual([])
})
