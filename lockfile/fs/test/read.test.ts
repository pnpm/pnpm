import path from 'path'
import { type DepPath, type ProjectId } from '@pnpm/types'
import { jest } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

jest.unstable_mockModule('@pnpm/git-utils', () => ({ getCurrentBranch: jest.fn() }))

const { getCurrentBranch } = await import('@pnpm/git-utils')
const {
  existsNonEmptyWantedLockfile,
  readCurrentLockfile,
  readWantedLockfile,
  writeCurrentLockfile,
  writeWantedLockfile,
} = await import('@pnpm/lockfile.fs')

process.chdir(import.meta.dirname)

test('readWantedLockfile()', async () => {
  {
    const lockfile = await readWantedLockfile(path.join('fixtures', '2'), {
      ignoreIncompatible: false,
    })
    expect(lockfile?.lockfileVersion).toBe('9.0')
    expect(lockfile?.importers).toStrictEqual({
      '.': {
        dependencies: {
          foo: '1.0.0',
        },
        devDependencies: undefined,
        optionalDependencies: undefined,
        specifiers: {
          foo: '1',
        },
        dependenciesMeta: {
          foo: { injected: true },
        },
      },
    })
  }

  try {
    await readWantedLockfile(path.join('fixtures', '3'), {
      ignoreIncompatible: false,
      wantedVersions: ['3'],
    })
    fail()
  } catch (err: any) { // eslint-disable-line
    expect(err.code).toBe('ERR_PNPM_LOCKFILE_BREAKING_CHANGE')
  }
})

test('readWantedLockfile() when lockfileVersion is a string', async () => {
  {
    const lockfile = await readWantedLockfile(path.join('fixtures', '4'), {
      ignoreIncompatible: false,
      wantedVersions: ['3'],
    })
    expect(lockfile!.lockfileVersion).toBe('v3')
  }

  {
    const lockfile = await readWantedLockfile(path.join('fixtures', '5'), {
      ignoreIncompatible: false,
      wantedVersions: ['3'],
    })
    expect(lockfile!.lockfileVersion).toBe('3')
  }
})

test('readCurrentLockfile()', async () => {
  const lockfile = await readCurrentLockfile('fixtures/2/node_modules/.pnpm', {
    ignoreIncompatible: false,
  })
  expect(lockfile!.lockfileVersion).toBe('6.0')
})

test('writeWantedLockfile()', async () => {
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
    lockfileVersion: '9.0',
    packages: {
      'is-negative@1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'is-positive@2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
    registry: 'https://registry.npmjs.org',
  }
  await writeWantedLockfile(projectPath, wantedLockfile)
  expect(await readCurrentLockfile(projectPath, { ignoreIncompatible: false })).toBeNull()
  expect(await readWantedLockfile(projectPath, { ignoreIncompatible: false })).toEqual(wantedLockfile)
})

test('writeCurrentLockfile()', async () => {
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
    lockfileVersion: '9.0',
    packages: {
      'is-negative@1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      'is-positive@2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
    registry: 'https://registry.npmjs.org',
  }
  await writeCurrentLockfile(projectPath, wantedLockfile)
  expect(await readWantedLockfile(projectPath, { ignoreIncompatible: false })).toBeNull()
  expect(await readCurrentLockfile(projectPath, { ignoreIncompatible: false })).toEqual(wantedLockfile)
})

test('existsNonEmptyWantedLockfile()', async () => {
  const projectPath = temporaryDirectory()
  expect(await existsNonEmptyWantedLockfile(projectPath)).toBe(false)
  await writeWantedLockfile(projectPath, {
    importers: {
      ['.' as ProjectId]: {
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
    lockfileVersion: '3',
    packages: {
      ['is-negative/1.0.0' as DepPath]: {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['is-positive/1.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      ['is-positive/2.0.0' as DepPath]: {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })
  expect(await existsNonEmptyWantedLockfile(projectPath)).toBe(true)
})

test('readWantedLockfile() when useGitBranchLockfile', async () => {
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('branch'))
  const lockfile = await readWantedLockfile(path.join('fixtures', '6'), {
    ignoreIncompatible: false,
  })
  expect(lockfile?.importers).toEqual({
    '.': {
      dependencies: {
        'is-positive': '1.0.0',
      },
      specifiers: {
        'is-positive': '1.0.0',
      },
    },
  })
  expect(lockfile?.packages).toStrictEqual({
    'is-positive@1.0.0': {
      resolution: {
        integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
      },
    },
  })

  const gitBranchLockfile = await readWantedLockfile(path.join('fixtures', '6'), {
    ignoreIncompatible: false,
    useGitBranchLockfile: true,
  })
  expect(gitBranchLockfile?.importers).toEqual({
    '.': {
      dependencies: {
        'is-positive': '2.0.0',
      },
      specifiers: {
        'is-positive': '2.0.0',
      },
    },
  })
  expect(gitBranchLockfile?.packages).toStrictEqual({
    'is-positive@2.0.0': {
      resolution: {
        integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
      },
    },
  })
})

test('readWantedLockfile() when useGitBranchLockfile and mergeGitBranchLockfiles', async () => {
  jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('branch'))
  const lockfile = await readWantedLockfile(path.join('fixtures', '6'), {
    ignoreIncompatible: false,
    useGitBranchLockfile: true,
    mergeGitBranchLockfiles: true,
  })
  expect(lockfile?.importers).toEqual({
    '.': {
      dependencies: {
        'is-positive': '2.0.0',
      },
      specifiers: {
        'is-positive': '2.0.0',
      },
    },
  })
  expect(lockfile?.packages).toStrictEqual({
    'is-positive@1.0.0': {
      resolution: {
        integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
      },
    },
    'is-positive@2.0.0': {
      resolution: {
        integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
      },
    },
  })
})

test('readWantedLockfile() with inlineSpecifiersFormat', async () => {
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
    packages: {
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
    registry: 'https://registry.npmjs.org',
  }

  const lockfile = await readWantedLockfile(path.join('fixtures', '7'), { ignoreIncompatible: false })
  expect(lockfile?.importers).toEqual(wantedLockfile.importers)
  expect(lockfile?.packages).toEqual(wantedLockfile.packages)
})
