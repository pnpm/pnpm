import assertProject from '@pnpm/assert-project'
import mkdirp = require('mkdirp')
import path = require('path')
import {Test} from 'tape'
import writePkg = require('write-pkg')

// the testing folder should be outside of the project to avoid lookup in the project's node_modules
const tmpPath = path.join(__dirname, '..', '..', '..', '..', '..', '.tmp')
mkdirp.sync(tmpPath)

let dirNumber = 0

export function tempDir (t: Test) {
  dirNumber++
  const dirname = dirNumber.toString()
  const tmpDir = path.join(tmpPath, dirname)
  mkdirp.sync(tmpDir)

  t.pass(`create testing dir ${dirname}`)

  process.chdir(tmpDir)

  return tmpDir
}

export default function prepare (t: Test, pkg?: Object | Object[], pkgTmpPath?: string): any {
  pkgTmpPath = pkgTmpPath || path.join(tempDir(t), 'project')

  if (Array.isArray(pkg)) {
    const dirname = path.dirname(pkgTmpPath)
    const result = {}
    for (let aPkg of pkg) {
      if (typeof aPkg['location'] === 'string') {
        result[aPkg['package']['name']] = prepare(t, aPkg['package'], path.join(dirname, aPkg['location']))
      } else {
        result[aPkg['name']] = prepare(t, aPkg, path.join(dirname, aPkg['name']))
      }
    }
    process.chdir('..')
    return result
  }
  mkdirp.sync(pkgTmpPath)
  writePkg.sync(pkgTmpPath, Object.assign({name: 'project', version: '0.0.0'}, pkg))
  process.chdir(pkgTmpPath)

  return assertProject(t, pkgTmpPath)
}
