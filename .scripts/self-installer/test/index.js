'use strict'
const test = require('tape')
const tempy = require('tempy')
const fs = require('fs')
const path = require('path')
const installTo = require('../src/installTo')
const isExecutable = require('isexe')
const isWindows = require('is-windows')()

const exeExtension = isWindows ? '.cmd' : ''

test('install', t => {
  const dest = tempy.directory()
  const binPath = path.join(dest, 'bin')
  fs.mkdirSync(binPath)
  installTo(dest, binPath)
    .then(() => {
      t.ok(isExecutable.sync(path.join(binPath, `pnpm${exeExtension}`)), 'pnpm is executable')
      t.ok(isExecutable.sync(path.join(binPath, `pnpx${exeExtension}`)), 'pnpx is executable')
      t.end()
    })
    .catch(t.end)
})
