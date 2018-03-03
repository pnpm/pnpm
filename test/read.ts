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
