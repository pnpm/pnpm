import assertProject, { Modules, Project } from '@pnpm/assert-project'
import { ImporterManifest } from '@pnpm/types'
import makeDir = require('make-dir')
import path = require('path')
import { Test } from 'tape'
import tempy = require('tempy')
import { sync as writeJson5File } from 'write-json5-file'
import writePkg = require('write-pkg')
import { sync as writeYamlFile } from 'write-yaml-file'

export { Modules, Project }
export type ManifestFormat = 'JSON' | 'JSON5' | 'YAML'

// the testing folder should be outside of the project to avoid lookup in the project's node_modules
const tmpPath = tempy.directory()

let dirNumber = 0

export function tempDir (t: Test) {
  dirNumber++
  const dirname = dirNumber.toString()
  const tmpDir = path.join(tmpPath, dirname)
  makeDir.sync(tmpDir)

  t.pass(`create testing dir ${path.join(tmpDir)}`)

  process.chdir(tmpDir)

  return tmpDir
}

export function preparePackages (
  t: Test,
  pkgs: Array<{ location: string, package: ImporterManifest } | ImporterManifest>,
  opts?: {
    manifestFormat?: ManifestFormat,
    tempDir?: string,
  }
) {
  const pkgTmpPath = opts?.tempDir ?? path.join(tempDir(t), 'project')
  const manifestFormat = opts?.manifestFormat

  const dirname = path.dirname(pkgTmpPath)
  const result: { [name: string]: Project } = {}
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
    manifestFormat?: ManifestFormat,
    tempDir?: string,
  }
) {
  const dir = opts?.tempDir ?? path.join(tempDir(test), 'project')

  makeDir.sync(dir)
  switch (opts?.manifestFormat ?? 'JSON') {
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
