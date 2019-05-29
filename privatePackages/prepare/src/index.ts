import assertProject from '@pnpm/assert-project'
import { Modules } from '@pnpm/modules-yaml'
import { ImporterManifest } from '@pnpm/types'
import makeDir = require('make-dir')
import path = require('path')
import { Test } from 'tape'
import writePkg = require('write-pkg')
import { sync as writeYamlFile } from 'write-yaml-file'
import { sync as writeJson5File } from 'write-json5-file'

// the testing folder should be outside of the project to avoid lookup in the project's node_modules
const tmpPath = path.join(__dirname, '..', '..', '..', '..', '.tmp')
makeDir.sync(tmpPath)

let dirNumber = 0

export function tempDir (t: Test) {
  dirNumber++
  const dirname = dirNumber.toString()
  const tmpDir = path.join(tmpPath, dirname)
  makeDir.sync(tmpDir)

  t.pass(`create testing dir ${dirname}`)

  process.chdir(tmpDir)

  return tmpDir
}

export function preparePackages (
  t: Test,
  pkgs: Array<{ location: string, package: ImporterManifest } | ImporterManifest>,
  opts?: {
    manifestFormat?: 'JSON' | 'JSON5' | 'YAML',
    tempDir?: string,
  }
): {
  [name: string]: {
    requireModule (pkgName: string): any // tslint:disable-line:no-any
    has (pkgName: string): Promise<void>
    hasNot (pkgName: string): Promise<void>
    getStorePath (): Promise<string>
    resolve (pkgName: string, version?: string | undefined, relativePath?: string | undefined): Promise<string>
    storeHas (pkgName: string, version?: string | undefined): Promise<void>
    storeHasNot (pkgName: string, version?: string | undefined): Promise<void>
    isExecutable (pathToExe: string): Promise<void>
    readCurrentLockfile (): Promise<any> // tslint:disable-line:no-any
    readModulesManifest: () => Promise<Modules | null>
    readLockfile (): Promise<any> // tslint:disable-line:no-any
    writePackageJson (pkgJson: object): Promise<void>
  }
} {
  const pkgTmpPath = opts && opts.tempDir || path.join(tempDir(t), 'project')
  const manifestFormat = opts && opts.manifestFormat

  const dirname = path.dirname(pkgTmpPath)
  const result = {}
  for (let aPkg of pkgs) {
    if (typeof aPkg['location'] === 'string') {
      result[aPkg['package']['name']] = prepare(t, aPkg['package'], {
        manifestFormat,
        tempDir: path.join(dirname, aPkg['location']),
      })
    } else {
      result[aPkg['name']] = prepare(t, aPkg as ImporterManifest, {
        manifestFormat,
        tempDir: path.join(dirname, aPkg['name']),
      })
    }
  }
  process.chdir('..')
  return result
}

export default function prepare (
  test: Test,
  manifest?: ImporterManifest,
  opts?: {
    manifestFormat?: 'JSON' | 'JSON5' | 'YAML',
    tempDir?: string,
  }
) {
  const dir = opts && opts.tempDir || path.join(tempDir(test), 'project')

  makeDir.sync(dir)
  switch (opts && opts.manifestFormat || 'JSON') {
    case 'JSON':
      writePkg.sync(dir, { name: 'project', version: '0.0.0', ...manifest } as any) // tslint:disable-line
      break
    case 'JSON5':
      writeJson5File(path.join(dir, 'package.json5'), { name: 'project', version: '0.0.0', ...manifest } as any) // tslint:disable-line
      break
    case 'YAML':
      writeYamlFile(path.join(dir, 'package.yaml'), { name: 'project', version: '0.0.0', ...manifest } as any) // tslint:disable-line
      break
  }
  process.chdir(dir)

  return assertProject(test, dir)
}

export function prepareEmpty (t: Test) {
  const pkgTmpPath = path.join(tempDir(t), 'project')

  makeDir.sync(pkgTmpPath)
  process.chdir(pkgTmpPath)

  return assertProject(t, pkgTmpPath)
}
