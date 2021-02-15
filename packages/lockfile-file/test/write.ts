import fs from 'fs'
import path from 'path'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import {
  readCurrentLockfile,
  readWantedLockfile,
  writeLockfiles,
} from '@pnpm/lockfile-file'
import tempy from 'tempy'
import yaml from 'yaml-tag'

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

test('writeLockfiles() does not fail if the lockfile has undefined properties', async () => {
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
        // eslint-disable-next-line
        dependencies: undefined as any,
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
})
