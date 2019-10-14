import { WANTED_LOCKFILE } from '@pnpm/constants'
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
import test = require('tape')
import tempy = require('tempy')
import yaml = require('yaml-tag')

process.chdir(__dirname)

test('readWantedLockfile()', async t => {
  {
    const lockfile = await readWantedLockfile(path.join('fixtures', '2'), {
      ignoreIncompatible: false,
    })
    t.equal(lockfile!.lockfileVersion, 3)
  }

  try {
    const lockfile = await readWantedLockfile(path.join('fixtures', '3'), {
      ignoreIncompatible: false,
      wantedVersion: 3,
    })
    t.fail()
  } catch (err) {
    t.equal(err.code, 'ERR_PNPM_LOCKFILE_BREAKING_CHANGE')
  }
  t.end()
})

test('readCurrentLockfile()', async t => {
  {
    const lockfile = await readCurrentLockfile('fixtures/2/node_modules/.pnpm', {
      ignoreIncompatible: false,
    })
    t.equal(lockfile!.lockfileVersion, 3)
  }
  t.end()
})

test('writeWantedLockfile()', async t => {
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
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
  }
  await writeWantedLockfile(projectPath, wantedLockfile)
  t.equal(await readCurrentLockfile(projectPath, { ignoreIncompatible: false }), null, 'current lockfile read')
  t.deepEqual(await readWantedLockfile(projectPath, { ignoreIncompatible: false }), wantedLockfile, 'wanted lockfile read')
  t.end()
})

test('writeCurrentLockfile()', async t => {
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
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
    registry: 'https://registry.npmjs.org',
  }
  await writeCurrentLockfile(projectPath, wantedLockfile)
  t.equal(await readWantedLockfile(projectPath, { ignoreIncompatible: false }), null)
  t.deepEqual(await readCurrentLockfile(projectPath, { ignoreIncompatible: false }), wantedLockfile)
  t.end()
})

test('existsWantedLockfile()', async t => {
  const projectPath = tempy.directory()
  t.notOk(await existsWantedLockfile(projectPath))
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
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        },
      },
    },
  })
  t.ok(await existsWantedLockfile(projectPath))
  t.end()
})

test('writeLockfiles()', async t => {
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
    lockfileVersion: 5.1,
    packages: {
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
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
  t.deepEqual(await readCurrentLockfile(projectPath, { ignoreIncompatible: false }), wantedLockfile)
  t.deepEqual(await readWantedLockfile(projectPath, { ignoreIncompatible: false }), wantedLockfile)
  t.end()
})

test('writeLockfiles() when no specifiers but dependencies present', async t => {
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
    lockfileVersion: 5.1,
  }
  await writeLockfiles({
    currentLockfile: wantedLockfile,
    currentLockfileDir: projectPath,
    wantedLockfile,
    wantedLockfileDir: projectPath,
  })
  t.deepEqual(await readCurrentLockfile(projectPath, { ignoreIncompatible: false }), wantedLockfile)
  t.deepEqual(await readWantedLockfile(projectPath, { ignoreIncompatible: false }), wantedLockfile)
  t.end()
})

test('write does not use yaml anchors/aliases', async t => {
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
    lockfileVersion: 5.1,
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
  t.ok(!lockfileContent.includes('&'), 'lockfile contains no anchors')
  t.ok(!lockfileContent.includes('*'), 'lockfile contains no aliases')

  t.end()
})
