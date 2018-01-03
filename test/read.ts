import {
  readWanted,
  readCurrent,
  readPrivate,
  read,
  write,
  writeWantedOnly,
} from 'pnpm-shrinkwrap'
import test = require('tape')
import path = require('path')
import tempy = require('tempy')

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
