import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import {
  existsWantedLockfile,
  readCurrentLockfile,
  readWantedLockfile,
  writeCurrentLockfile,
  writeLockfiles,
  writeWantedLockfile,
} from '@pnpm/lockfile-file'
import fs = require('fs')
import path = require('path')
import tempy = require('tempy')
import yaml = require('yaml-tag')

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

test('writeLockfiles()', async () => {
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
    lockfileVersion: LOCKFILE_VERSION,
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

test('writeLockfiles() when no specifiers but dependencies present', async () => {
  const projectPath = tempy.directory()
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
  const projectPath = tempy.directory()
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
      /react-dnd/2.5.4/react@15.6.1:
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
      /react-dnd/2.5.4/react@15.6.2:
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
