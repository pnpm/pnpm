import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  prepare,
  addDistTag,
  execPnpm
} from '../utils'
import fs = require('mz/fs')

const test = promisifyTape(tape)

test('readPackage hook', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('pnpmfile.js', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm('install', 'pkg-with-1-dep')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})
