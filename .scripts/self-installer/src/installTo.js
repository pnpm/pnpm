'use strict'
const spawnSync = require('child_process').spawnSync
const path = require('path')
const fs = require('fs')
const got = require('got')
const unpackStream = require('unpack-stream')
const getNpmTarballUrl = require('get-npm-tarball-url').default
const cmdShim = require('cmd-shim')
const pnpmPkg = require('../../../package.json')

module.exports = installTo

function installTo (dest, binPath) {
  const pnpm = path.join(dest, 'pnpm')
  const npmBin = path.join(pnpm, 'node_modules', 'not-bundled-npm', 'bin', 'npm-cli')
  const pnpmBin = path.join(pnpm, 'lib/bin/pnpm.js')
  const pnpxBin = path.join(pnpm, 'lib/bin/pnpx.js')

  const tarball = getNpmTarballUrl(pnpmPkg.bundledName || pnpmPkg.name, pnpmPkg.version)
  console.log(`Downloading ${tarball}`)
  const stream = got.stream(tarball)
  return unpackStream.remote(stream, pnpm)
    .then(index => {
      return new Promise((resolve, reject) => {
        cmdShim(pnpmBin, path.join(binPath, 'pnpm'), (err) => {
          if (err) {
            reject(err)
            return
          }
          cmdShim(pnpxBin, path.join(binPath, 'pnpx'), (err) => {
            if (err) {
              reject(err)
              return
            }
            spawnSync('node', [npmBin, 'rebuild', 'drivelist'], {cwd: pnpm, stdio: 'inherit'})
            resolve()
          })
        })
      })
    })
}
