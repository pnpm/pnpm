import path from 'path'
import { getCurrentBranch } from '@pnpm/git-utils'
import {
  existsWantedLockfile,
  readCurrentLockfile,
  readWantedLockfile,
  writeCurrentLockfile,
  writeWantedLockfile,
} from '@pnpm/lockfile-file'
import tempy from 'tempy'

jest.mock('@pnpm/git-utils', () => ({ getCurrentBranch: jest.fn() }))

process.chdir(__dirname)

test('readWantedLockfile()', async () => {
  {
    const lockfile = await readWantedLockfile(path.join('fixtures', '2'), {
      ignoreIncompatible: false,
    })
    expect(lockfile?.lockfileVersion).toEqual(3)
    expect(lockfile?.importers).toStrictEqual({
      '.': {
        specifiers: {
          foo: '1',
        },
        dependenciesMeta: {
          foo: { injected: true },
        },
        publishDirectory: undefined,
      },
    })
  }

  try {
    await readWantedLockfile(path.join('fixtures', '3'), {
      ignoreIncompatible: false,
      wantedVersion: 3,
    })
    fail()
  } catch (err: any) { // eslint-disable-line
    expect(err.code).toEqual('ERR_PNPM_LOCKFILE_BREAKING_CHANGE')
  }
})

test('readWantedLockfile() when lockfileVersion is a string', async () => {
  {
    const lockfile = await readWantedLockfile(path.join('fixtures', '4'), {
      ignoreIncompatible: false,
      wantedVersion: 3,
    })
    expect(lockfile!.lockfileVersion).toEqual('v3')
  }

  {
    const lockfile = await readWantedLockfile(path.join('fixtures', '5'), {
      ignoreIncompatible: false,
      wantedVersion: 3,
    })
    expect(lockfile!.lockfileVersion).toEqual('3')
  }
})

test('readCurrentLockfile()', async () => {
  const lockfile = await readCurrentLockfile('fixtures/2/node_modules/.pnpm', {
    ignoreIncompatible: false,
  })
  expect(lockfile!.lockfileVersion).toEqual(3)
})

test('writeWantedLockfile()', async () => {
  const projectPath = tempy.directory()
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
    lockfileVersion: 3,
    packages: {
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/2.0.0': {
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
  const projectPath = tempy.directory()
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
    lockfileVersion: 3,
    packages: {
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/2.0.0': {
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

test('existsWantedLockfile()', async () => {
  const projectPath = tempy.directory()
  expect(await existsWantedLockfile(projectPath)).toBe(false)
  await writeWantedLockfile(projectPath, {
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
    lockfileVersion: 3,
    packages: {
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
        },
      },
    },
  })
  expect(await existsWantedLockfile(projectPath)).toBe(true)
})

test('readWantedLockfile() when useGitBranchLockfile', async () => {
  getCurrentBranch['mockReturnValue']('branch')
  const lockfile = await readWantedLockfile(path.join('fixtures', '6'), {
    ignoreIncompatible: false,
  })
  expect(lockfile?.importers).toEqual({
    '.': {
      specifiers: {
        'is-positive': '1.0.0',
      },
    },
  })
  expect(lockfile?.packages).toStrictEqual({
    '/is-positive/1.0.0': {
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
      specifiers: {
        'is-positive': '2.0.0',
      },
    },
  })
  expect(gitBranchLockfile?.packages).toStrictEqual({
    '/is-positive/2.0.0': {
      resolution: {
        integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
      },
    },
  })
})

test('readWantedLockfile() when useGitBranchLockfile and mergeGitBranchLockfiles', async () => {
  getCurrentBranch['mockReturnValue']('branch')
  const lockfile = await readWantedLockfile(path.join('fixtures', '6'), {
    ignoreIncompatible: false,
    useGitBranchLockfile: true,
    mergeGitBranchLockfiles: true,
  })
  expect(lockfile?.importers).toEqual({
    '.': {
      specifiers: {
        'is-positive': '2.0.0',
      },
    },
  })
  expect(lockfile?.packages).toStrictEqual({
    '/is-positive/1.0.0': {
      resolution: {
        integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
      },
    },
    '/is-positive/2.0.0': {
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
      '/is-positive/1.0.0': {
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
