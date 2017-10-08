import {
  readWanted,
  readCurrent,
  readPrivate,
  read,
} from '../src'
import test = require('tape')
import path = require('path')

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
