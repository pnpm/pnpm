'use strict'
const spawnSync = require('child_process').spawnSync
const path = require('path')
const fs = require('fs')
const execPath = process.execPath
const binPath = path.dirname(execPath)
const pnpm = path.join(execPath, '../../lib/node_modules/pnpm')
const npmBin = path.join(pnpm, 'node_modules', 'not-bundled-npm', 'bin', 'npm-cli')
const bin = path.join(pnpm, 'lib/bin/pnpm.js')
const got = require('got')
const unpackStream = require('unpack-stream')
const getNpmTarballUrl = require('get-npm-tarball-url').default
const cmdShim = require('cmd-shim')
const pnpmPkg = require('../../../package.json')

const tarball = getNpmTarballUrl(pnpmPkg.bundledName || pnpmPkg.name, pnpmPkg.version)
console.log(`Downloading ${tarball}`)
const stream = got.stream(tarball)
unpackStream.remote(stream, pnpm)
  .then(index => {
    process.stdout.write('link: ' + bin + '\n')
    process.stdout.write(' => ' + path.join(binPath, 'pnpm') + '\n')
    cmdShim(bin, path.join(binPath, 'pnpm'), (err) => {
      if (err) throw err

      spawnSync('node', [npmBin, 'rebuild', 'drivelist'], {cwd: pnpm, stdio: 'inherit'})
    })
  })
  .catch(err => {
    console.error(err)
    process.exit(1)
  })
