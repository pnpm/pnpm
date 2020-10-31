import {
  existsWantedLockfile,
  readCurrentLockfile,
  readWantedLockfile,
  writeCurrentLockfile,
  writeWantedLockfile,
} from '@pnpm/lockfile-file'
import path = require('path')
import tempy = require('tempy')

process.chdir(__dirname)

test('readWantedLockfile()', async () => {
  {
    const lockfile = await readWantedLockfile(path.join('fixtures', '2'), {
      ignoreIncompatible: false,
    })
    expect(lockfile!.lockfileVersion).toEqual(3)
  }

  try {
    await readWantedLockfile(path.join('fixtures', '3'), {
      ignoreIncompatible: false,
      wantedVersion: 3,
    })
    fail()
  } catch (err) {
    expect(err.code).toEqual('ERR_PNPM_LOCKFILE_BREAKING_CHANGE')
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
