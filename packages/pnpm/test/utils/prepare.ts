import assertProject from '@pnpm/assert-project'
import {Modules} from '@pnpm/modules-yaml'
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

export function preparePackages (
  t: Test,
  pkgs: | Object[], pkgTmpPath?: string,
): {
  [name: string]: {
    requireModule(pkgName: string): any;
    has(pkgName: string): Promise<void>;
    hasNot(pkgName: string): Promise<void>;
    getStorePath(): Promise<string>;
    resolve(pkgName: string, version?: string | undefined, relativePath?: string | undefined): Promise<string>;
    storeHas(pkgName: string, version?: string | undefined): Promise<void>;
    storeHasNot(pkgName: string, version?: string | undefined): Promise<void>;
    isExecutable(pathToExe: string): Promise<void>;
    loadCurrentShrinkwrap(): Promise<any>;
    loadModules: () => Promise<Modules | null>;
    loadShrinkwrap(): Promise<any>;
    writePackageJson(pkgJson: object): Promise<void>;
  }
} {
  pkgTmpPath = pkgTmpPath || path.join(tempDir(t), 'project')

  const dirname = path.dirname(pkgTmpPath)
  const result = {}
  for (let aPkg of pkgs) {
    if (typeof aPkg['location'] === 'string') {
      result[aPkg['package']['name']] = prepare(t, aPkg['package'], path.join(dirname, aPkg['location']))
    } else {
      result[aPkg['name']] = prepare(t, aPkg, path.join(dirname, aPkg['name']))
    }
  }
  process.chdir('..')
  return result
}

export default function prepare (t: Test, pkg?: Object, pkgTmpPath?: string) {
  pkgTmpPath = pkgTmpPath || path.join(tempDir(t), 'project')

  mkdirp.sync(pkgTmpPath)
  writePkg.sync(pkgTmpPath, Object.assign({name: 'project', version: '0.0.0'}, pkg))
  process.chdir(pkgTmpPath)

  return assertProject(t, pkgTmpPath)
}
