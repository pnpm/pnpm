import crossSpawn = require('cross-spawn')
import path = require('path')
import delocalizeDeps = require('delocalize-dependencies')
import readPkgUp = require('read-pkg-up')
import {PublicInstallationOptions} from '../api/install'
import writeJson from '../fs/writeJson'

export default async function (input: string[], opts: PublicInstallationOptions) {
  if (!opts.linkLocal) {
    runNpmPublish()
  }
  const pkg = await readPkgUp({ cwd: opts.cwd, normalize: false })
  const newPkg = delocalizeDeps({
    saveExact: opts.saveExact,
    pkgDir: path.dirname(pkg.path),
    pkg: pkg.pkg
  })
  await writeJson(pkg.path, newPkg)
  try {
    await runNpmPublish()
  } finally {
    await writeJson(pkg.path, pkg.pkg)
  }
}

function runNpmPublish () {
  crossSpawn.sync('npm', process.argv.slice(2), { stdio: 'inherit' })
}
