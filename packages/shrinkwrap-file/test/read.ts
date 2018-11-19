import {
  existsWanted,
  read,
  readCurrent,
  readPrivate,
  readWanted,
  write,
  writeCurrentOnly,
  writeWantedOnly,
} from '@pnpm/shrinkwrap-file'
import { Shrinkwrap } from '@pnpm/shrinkwrap-types'
import fs = require('fs')
import mkdirp = require('mkdirp-promise')
import path = require('path')
import readYamlFile from 'read-yaml-file'
import test = require('tape')
import tempy = require('tempy')
import writeYamlFile = require('write-yaml-file')
import yaml = require('yaml-tag')

process.chdir(__dirname)

test('backward compatibility', t => {
  t.equal(readWanted, read)
  t.equal(readCurrent, readPrivate)
  t.end()
})

test('readWanted()', async t => {
  {
    const shr = await readWanted(path.join('fixtures', '1'), {
      ignoreIncompatible: false,
    })
    t.equal(shr!.shrinkwrapVersion, 3, 'converted version to shrinkwrapVersion')
  }
  {
    const shr = await readWanted(path.join('fixtures', '2'), {
      ignoreIncompatible: false,
    })
    t.equal(shr!.shrinkwrapVersion, 3)
  }

  try {
    const shr = await readWanted(path.join('fixtures', '3'), {
      ignoreIncompatible: false,
      wantedVersion: 3,
    })
    t.fail()
  } catch (err) {
    t.equal(err.code, 'SHRINKWRAP_BREAKING_CHANGE')
  }
  t.end()
})

test('readCurrent()', async t => {
  {
    const shr = await readCurrent(path.join('fixtures', '1'), {
      ignoreIncompatible: false,
    })
    t.equal(shr!.shrinkwrapVersion, 3, 'converted version to shrinkwrapVersion')
  }
  {
    const shr = await readCurrent(path.join('fixtures', '2'), {
      ignoreIncompatible: false,
    })
    t.equal(shr!.shrinkwrapVersion, 3)
  }
  t.end()
})

test('writeWantedOnly()', async t => {
  const projectPath = tempy.directory()
  const wantedShrinkwrap = {
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
    shrinkwrapVersion: 3,
  }
  await writeWantedOnly(projectPath, wantedShrinkwrap)
  t.equal(await readCurrent(projectPath, { ignoreIncompatible: false }), null, 'current shrinkwrap read')
  t.deepEqual(await readWanted(projectPath, { ignoreIncompatible: false }), wantedShrinkwrap, 'wanted shrinkwrap read')
  t.end()
})

test('writeCurrentOnly()', async t => {
  const projectPath = tempy.directory()
  const wantedShrinkwrap = {
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
    shrinkwrapVersion: 3,
  }
  await mkdirp(path.join(projectPath, 'node_modules'))
  await writeCurrentOnly(projectPath, wantedShrinkwrap)
  t.equal(await readWanted(projectPath, { ignoreIncompatible: false }), null)
  t.deepEqual(await readCurrent(projectPath, { ignoreIncompatible: false }), wantedShrinkwrap)
  t.end()
})

test('existsWanted()', async t => {
  const projectPath = tempy.directory()
  t.notOk(await existsWanted(projectPath))
  await writeWantedOnly(projectPath, {
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
    shrinkwrapVersion: 3,
  })
  t.ok(await existsWanted(projectPath))
  t.end()
})

test('write()', async t => {
  const projectPath = tempy.directory()
  const wantedShrinkwrap = {
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
    shrinkwrapVersion: 3,
  }
  await write(projectPath, wantedShrinkwrap, wantedShrinkwrap)
  t.deepEqual(await readCurrent(projectPath, { ignoreIncompatible: false }), wantedShrinkwrap)
  t.deepEqual(await readWanted(projectPath, { ignoreIncompatible: false }), wantedShrinkwrap)
  t.end()
})

test('write() when no specifiers but dependencies present', async t => {
  const projectPath = tempy.directory()
  const wantedShrinkwrap = {
    importers: {
      '.': {
        dependencies: {
          'is-positive': 'link:../is-positive',
        },
        specifiers: {},
      },
    },
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  }
  await write(projectPath, wantedShrinkwrap, wantedShrinkwrap)
  t.deepEqual(await readCurrent(projectPath, { ignoreIncompatible: false }), wantedShrinkwrap)
  t.deepEqual(await readWanted(projectPath, { ignoreIncompatible: false }), wantedShrinkwrap)
  t.end()
})

test('write does not use yaml anchors/aliases', async t => {
  const projectPath = tempy.directory()
  const wantedShrinkwrap = {
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
    registry: 'https://registry.npmjs.org',
    shrinkwrapVersion: 3,
  }
  await write(projectPath, wantedShrinkwrap, wantedShrinkwrap)

  const shrContent = fs.readFileSync(path.join(projectPath, 'shrinkwrap.yaml'), 'utf8')
  t.ok(shrContent.indexOf('&') === -1, 'shrinkwrap contains no anchors')
  t.ok(shrContent.indexOf('*') === -1, 'shrinkwrap contains no aliases')

  t.end()
})

test('read merges minor and major shrinkwrap versions', async t => {
  const projectPath = tempy.directory()
  const wantedShrinkwrap = {
    registry: 'https://registry.npmjs.org',
    shrinkwrapMinorVersion: 11,
    shrinkwrapVersion: 3,
  }
  await writeYamlFile(path.join(projectPath, 'shrinkwrap.yaml'), wantedShrinkwrap)

  const shr = await readWanted(projectPath, { ignoreIncompatible: true })
  t.equals(shr && shr.shrinkwrapVersion, 3.11)

  t.end()
})

test('write saves shrinkwrap version in correct fields', async t => {
  const projectPath = tempy.directory()
  await writeWantedOnly(projectPath, {
    importers: {
      '.': {
        dependencies: {
          foo: '1.0.0',
        },
        specifiers: {
          foo: '1.0.0',
        },
      },
    },
    packages: {
      '/foo/1.0.0': {
        resolution: {
          integrity: 'aaa',
        },
      },
    },
    registry: 'https://registry.npmjs.org/',
    shrinkwrapVersion: 3.11,
  })
  const shr = await readYamlFile<Shrinkwrap>(path.join(projectPath, 'shrinkwrap.yaml'))
  t.equal(shr['shrinkwrapVersion'], 3)
  t.equal(shr['shrinkwrapMinorVersion'], 11)
  t.end()
})
