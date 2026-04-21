import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import type { ProjectId } from '@pnpm/types'
import { temporaryDirectory } from 'tempy'
import yaml from 'yaml-tag'

jest.unstable_mockModule('@pnpm/network.git-utils', () => ({ getCurrentBranch: jest.fn() }))

const { getCurrentBranch } = await import('@pnpm/network.git-utils')
const {
  readCurrentLockfile,
  readWantedLockfile,
  writeLockfiles,
  writeWantedLockfile,
} = await import('@pnpm/lockfile.fs')

test('writeLockfiles()', async () => {
  const projectPath = temporaryDirectory()
  const wantedLockfile = {
    importers: {
      '.': {
        dependencies: {
          'is-negative': '1.0.0',
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-negative': '^1.0.0',
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative@1.0.0': {
        os: ['darwin'],
        dependencies: {
          'is-positive': '2.0.0',
        },
        cpu: ['x86'],
        libc: ['glibc'],
        engines: {
          node: '>=10',
          npm: '\nfoo\n',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive@2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }
  await writeLockfiles({
    currentLockfile: wantedLockfile,
    currentLockfileDir: projectPath,
    wantedLockfile,
    wantedLockfileDir: projectPath,
  })
  expect(await readCurrentLockfile(projectPath, { ignoreIncompatible: false })).toEqual(wantedLockfile)
  expect(await readWantedLockfile(projectPath, { ignoreIncompatible: false })).toEqual(wantedLockfile)

  // Verifying the formatting of the lockfile
  expect(fs.readFileSync(path.join(projectPath, WANTED_LOCKFILE), 'utf8')).toMatchSnapshot()
})

test('writeLockfiles() when no specifiers but dependencies present', async () => {
  const projectPath = temporaryDirectory()
  const wantedLockfile = {
    importers: {
      '.': {
        dependencies: {
          'is-positive': 'link:../is-positive',
        },
        specifiers: {},
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {},
  }
  await writeLockfiles({
    currentLockfile: wantedLockfile,
    currentLockfileDir: projectPath,
    wantedLockfile,
    wantedLockfileDir: projectPath,
  })
  expect(await readCurrentLockfile(projectPath, { ignoreIncompatible: false })).toEqual(wantedLockfile)
  expect(await readWantedLockfile(projectPath, { ignoreIncompatible: false })).toEqual(wantedLockfile)
})

test('write does not use yaml anchors/aliases', async () => {
  const projectPath = temporaryDirectory()
  const wantedLockfile = {
    importers: {
      '.': {
        dependencies: {
          'is-negative': '1.0.0',
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-negative': '1.0.0',
          'is-positive': '1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: yaml`
      /react-dnd@2.5.4(react@15.6.1):
        dependencies:
          disposables: 1.0.2
          dnd-core: 2.5.4
          hoist-non-react-statics: 2.5.0
          invariant: 2.2.3
          lodash: 4.15.0
          prop-types: 15.6.1
          react: 15.6.1
        dev: false
        id: registry.npmjs.org/react-dnd/2.5.4
        peerDependencies: &ref_11
          react: '1'
        resolution:
          integrity: sha512-y9YmnusURc+3KPgvhYKvZ9oCucj51MSZWODyaeV0KFU0cquzA7dCD1g/OIYUKtNoZ+MXtacDngkdud2TklMSjw==
      /react-dnd@2.5.4(react@15.6.2):
        dependencies:
          disposables: 1.0.2
          dnd-core: 2.5.4
          hoist-non-react-statics: 2.5.0
          invariant: 2.2.3
          lodash: 4.15.0
          prop-types: 15.6.1
          react: 15.6.2
        dev: false
        id: registry.npmjs.org/react-dnd/2.5.4
        peerDependencies: *ref_11
        resolution:
          integrity: sha512-y9YmnusURc+3KPgvhYKvZ9oCucj51MSZWODyaeV0KFU0cquzA7dCD1g/OIYUKtNoZ+MXtacDngkdud2TklMSjw==
    `,
  }
  await writeLockfiles({
    currentLockfile: wantedLockfile,
    currentLockfileDir: projectPath,
    wantedLockfile,
    wantedLockfileDir: projectPath,
  })

  const lockfileContent = fs.readFileSync(path.join(projectPath, WANTED_LOCKFILE), 'utf8')
  expect(lockfileContent).not.toMatch('&')
  expect(lockfileContent).not.toMatch('*')
})

test('writeLockfiles() does not fail if the lockfile has undefined properties', async () => {
  const projectPath = temporaryDirectory()
  const wantedLockfile = {
    importers: {
      '.': {
        dependencies: {
          'is-negative': '1.0.0',
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-negative': '^1.0.0',
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative@1.0.0': {
        // eslint-disable-next-line
        dependencies: undefined as any,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive@2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }
  await writeLockfiles({
    currentLockfile: wantedLockfile,
    currentLockfileDir: projectPath,
    wantedLockfile,
    wantedLockfileDir: projectPath,
  })
})

test('writeLockfiles() when useGitBranchLockfile', async () => {
  const branchName: string = 'branch'
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve(branchName))
  const projectPath = temporaryDirectory()
  const wantedLockfile = {
    importers: {
      '.': {
        dependencies: {
          foo: '1.0.0',
        },
        specifiers: {
          foo: '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/foo@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }

  await writeLockfiles({
    currentLockfile: wantedLockfile,
    currentLockfileDir: projectPath,
    wantedLockfile,
    wantedLockfileDir: projectPath,
    useGitBranchLockfile: true,
  })
  expect(fs.existsSync(path.join(projectPath, WANTED_LOCKFILE))).toBeFalsy()
  expect(fs.existsSync(path.join(projectPath, `pnpm-lock.${branchName}.yaml`))).toBeTruthy()
})

test('writeLockfiles() preserves env document prefix in pnpm-lock.yaml', async () => {
  const projectPath = temporaryDirectory()
  const envDoc = '---\nlockfileVersion: env-1.0\nimporters:\n  .:\n    configDependencies:\n      typescript: 5.0.0\n\n---\n'
  const wantedLockfile = {
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }

  // Write lockfile with an env document prefix already present
  fs.writeFileSync(path.join(projectPath, WANTED_LOCKFILE), envDoc + 'lockfileVersion: "9.0"\n')

  await writeLockfiles({
    currentLockfile: wantedLockfile,
    currentLockfileDir: projectPath,
    wantedLockfile,
    wantedLockfileDir: projectPath,
  })

  const written = fs.readFileSync(path.join(projectPath, WANTED_LOCKFILE), 'utf8')
  // The env document should be preserved at the top
  expect(written.startsWith('---\n')).toBe(true)
  expect(written).toContain('configDependencies')
  expect(written).toContain('typescript: 5.0.0')

  // The main lockfile should still be readable
  const lockfile = await readWantedLockfile(projectPath, { ignoreIncompatible: false })
  expect(lockfile).toBeTruthy()
  expect(lockfile!.importers['.' as ProjectId].dependencies).toEqual({ 'is-positive': '1.0.0' })
})

test('writeWantedLockfile() preserves env document prefix', async () => {
  const projectPath = temporaryDirectory()
  const envDoc = '---\nlockfileVersion: env-1.0\nimporters:\n  .:\n    configDependencies:\n      typescript: 5.0.0\n\n---\n'
  const wantedLockfile = {
    importers: {
      '.': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        specifiers: {
          'is-positive': '^1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  }

  // Pre-seed with env document
  fs.writeFileSync(path.join(projectPath, WANTED_LOCKFILE), envDoc + 'lockfileVersion: "9.0"\n')

  await writeWantedLockfile(projectPath, wantedLockfile)

  const written = fs.readFileSync(path.join(projectPath, WANTED_LOCKFILE), 'utf8')
  expect(written.startsWith('---\n')).toBe(true)
  expect(written).toContain('typescript: 5.0.0')

  // Main lockfile should be readable
  const lockfile = await readWantedLockfile(projectPath, { ignoreIncompatible: false })
  expect(lockfile!.importers['.' as ProjectId].dependencies).toEqual({ 'is-positive': '1.0.0' })
})

test('readWantedLockfile() skips env document in combined lockfile', async () => {
  const projectPath = temporaryDirectory()
  const envDoc = '---\nlockfileVersion: env-1.0\nimporters:\n  .:\n    configDependencies:\n      typescript: 5.0.0\n\n---\n'
  const mainDoc = `lockfileVersion: '${LOCKFILE_VERSION}'
importers:
  .:
    dependencies:
      is-positive:
        version: 1.0.0
        specifier: ^1.0.0
packages:
  is-positive@1.0.0:
    resolution:
      integrity: sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=
`
  fs.writeFileSync(path.join(projectPath, WANTED_LOCKFILE), envDoc + mainDoc)

  const lockfile = await readWantedLockfile(projectPath, { ignoreIncompatible: false })
  expect(lockfile).toBeTruthy()
  expect(lockfile!.lockfileVersion).toBe(LOCKFILE_VERSION)
  expect(lockfile!.importers['.' as ProjectId].dependencies).toEqual({ 'is-positive': '1.0.0' })
})

test('readWantedLockfile() returns null for env-only lockfile with no main document', async () => {
  const projectPath = temporaryDirectory()
  fs.writeFileSync(path.join(projectPath, WANTED_LOCKFILE), '---\nlockfileVersion: env-1.0\n')

  const lockfile = await readWantedLockfile(projectPath, { ignoreIncompatible: false })
  expect(lockfile).toBeNull()
})
