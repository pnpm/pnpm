import {
  existsWanted,
  readWanted,
  readCurrent,
  readPrivate,
  read,
  write,
  writeWantedOnly,
  writeCurrentOnly,
} from 'pnpm-shrinkwrap'
import test = require('tape')
import path = require('path')
import tempy = require('tempy')
import mkdirp = require('mkdirp-promise')
import yaml = require('yaml-tag')
import fs = require('fs')

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
    shrinkwrapVersion: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  }
  await writeWantedOnly(projectPath, wantedShrinkwrap)
  t.equal(await readCurrent(projectPath, {ignoreIncompatible: false}), null)
  t.deepEqual(await readWanted(projectPath, {ignoreIncompatible: false}), wantedShrinkwrap)
  t.end()
})

test('writeCurrentOnly()', async t => {
  const projectPath = tempy.directory()
  const wantedShrinkwrap = {
    shrinkwrapVersion: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  }
  await mkdirp(path.join(projectPath, 'node_modules'))
  await writeCurrentOnly(projectPath, wantedShrinkwrap)
  t.equal(await readWanted(projectPath, {ignoreIncompatible: false}), null)
  t.deepEqual(await readCurrent(projectPath, {ignoreIncompatible: false}), wantedShrinkwrap)
  t.end()
})

test('existsWanted()', async t => {
  const projectPath = tempy.directory()
  t.notOk(await existsWanted(projectPath))
  await writeWantedOnly(projectPath, {
    shrinkwrapVersion: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  })
  t.ok(await existsWanted(projectPath))
  t.end()
})

test('write()', async t => {
  const projectPath = tempy.directory()
  const wantedShrinkwrap = {
    shrinkwrapVersion: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  }
  await write(projectPath, wantedShrinkwrap, wantedShrinkwrap)
  t.deepEqual(await readCurrent(projectPath, {ignoreIncompatible: false}), wantedShrinkwrap)
  t.deepEqual(await readWanted(projectPath, {ignoreIncompatible: false}), wantedShrinkwrap)
  t.end()
})

test('write() when no specifiers but dependencies present', async t => {
  const projectPath = tempy.directory()
  const wantedShrinkwrap = {
    shrinkwrapVersion: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'is-positive': 'link:../is-positive',
    },
    specifiers: {},
  }
  await write(projectPath, wantedShrinkwrap, wantedShrinkwrap)
  t.deepEqual(await readCurrent(projectPath, {ignoreIncompatible: false}), wantedShrinkwrap)
  t.deepEqual(await readWanted(projectPath, {ignoreIncompatible: false}), wantedShrinkwrap)
  t.end()
})

test("write does not use yaml anchors/aliases", async t => {
  const projectPath = tempy.directory()
  const wantedShrinkwrap = {
    shrinkwrapVersion: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
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
    specifiers: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
  }
  await write(projectPath, wantedShrinkwrap, wantedShrinkwrap)

  const shrContent = fs.readFileSync(path.join(projectPath, 'shrinkwrap.yaml'), 'utf8')
  t.ok(shrContent.indexOf('&') === -1, 'shrinkwrap contains no anchors')
  t.ok(shrContent.indexOf('*') === -1, 'shrinkwrap contains no aliases')

  t.end()
})
