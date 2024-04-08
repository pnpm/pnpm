import fs from 'fs'
import path from 'path'
import { assertProject, type Modules, type Project } from '@pnpm/assert-project'
import { type ProjectManifest } from '@pnpm/types'
import { sync as writeJson5File } from 'write-json5-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import writePkg from 'write-pkg'

export type { Modules, Project }
export type ManifestFormat = 'JSON' | 'JSON5' | 'YAML'

// The testing folder should be outside of the project to avoid lookup in the project's node_modules
// Not using the OS temp directory due to issues on Windows CI.
const tmpBaseDir = path.join(__dirname, '../../../../pnpm_tmp')

function getFilesCountInDir (dir: string): number {
  try {
    return fs.readdirSync(dir).length
  } catch {
    return 0
  }
}

const tmpPath = path.join(tmpBaseDir, `${getFilesCountInDir(tmpBaseDir).toString()}_${process.pid.toString()}`)

let dirNumber = 0

export function tempDir (chdir: boolean = true) {
  dirNumber++
  const dirname = dirNumber.toString()
  const tmpDir = path.join(tmpPath, dirname)
  fs.mkdirSync(tmpDir, { recursive: true })

  if (chdir) process.chdir(tmpDir)

  return tmpDir
}

interface LocationAndManifest {
  location: string
  package: ProjectManifest
}

export function preparePackages (
  pkgs: Array<LocationAndManifest | ProjectManifest>,
  opts?: {
    manifestFormat?: ManifestFormat
    tempDir?: string
  }
) {
  const pkgTmpPath = opts?.tempDir ?? path.join(tempDir(), 'project')
  const manifestFormat = opts?.manifestFormat

  const dirname = path.dirname(pkgTmpPath)
  const result: { [name: string]: Project } = {}
  const cwd = process.cwd()
  for (const aPkg of pkgs) {
    if (typeof (aPkg as LocationAndManifest).location === 'string') {
      result[(aPkg as LocationAndManifest).package.name!] = prepare((aPkg as LocationAndManifest).package, {
        manifestFormat,
        tempDir: path.join(dirname, (aPkg as LocationAndManifest).location),
      })
    } else {
      result[(aPkg as ProjectManifest).name!] = prepare(aPkg as ProjectManifest, {
        manifestFormat,
        tempDir: path.join(dirname, (aPkg as ProjectManifest).name!),
      })
    }
  }
  process.chdir(cwd)
  return result
}

export function prepare (
  manifest?: ProjectManifest,
  opts?: {
    manifestFormat?: ManifestFormat
    tempDir?: string
  }
) {
  const dir = opts?.tempDir ?? path.join(tempDir(), 'project')

  fs.mkdirSync(dir, { recursive: true })
  switch (opts?.manifestFormat ?? 'JSON') {
  case 'JSON':
      writePkg.sync(dir, { name: 'project', version: '0.0.0', ...manifest } as any) // eslint-disable-line
    break
  case 'JSON5':
      writeJson5File(path.join(dir, 'package.json5'), { name: 'project', version: '0.0.0', ...manifest } as any) // eslint-disable-line
    break
  case 'YAML':
      writeYamlFile(path.join(dir, 'package.yaml'), { name: 'project', version: '0.0.0', ...manifest } as any) // eslint-disable-line
    break
  }
  process.chdir(dir)

  return assertProject(dir)
}

export function prepareEmpty () {
  const pkgTmpPath = path.join(tempDir(), 'project')

  fs.mkdirSync(pkgTmpPath, { recursive: true })
  process.chdir(pkgTmpPath)

  return assertProject(pkgTmpPath)
}
