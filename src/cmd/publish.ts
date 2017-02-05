import crossSpawn = require('cross-spawn')
import path = require('path')
import delocalizeDeps = require('delocalize-dependencies')
import readPkgUp = require('read-pkg-up')
import {PnpmOptions} from '../types'
import writePkg = require('write-pkg')
import verifyCmd from './verify'

export default async function (input: string[], opts: PnpmOptions) {
  await verifyCmd(input, opts)

  if (!opts.linkLocal) {
    runNpmPublish()
  }
  const pkg = await readPkgUp({ cwd: opts.cwd, normalize: false })
  const newPkg = delocalizeDeps({
    saveExact: opts.saveExact,
    pkgDir: path.dirname(pkg.path),
    pkg: pkg.pkg
  })
  await writePkg(pkg.path, newPkg)
  try {
    await runNpmPublish()
  } finally {
    await writePkg(pkg.path, pkg.pkg)
  }
}

function runNpmPublish () {
  crossSpawn.sync('npm', process.argv.slice(2), { stdio: 'inherit' })
}
