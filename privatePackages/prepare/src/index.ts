import fs from 'fs'
import path from 'path'
import assertProject, { Modules, Project } from '@pnpm/assert-project'
import { ProjectManifest } from '@pnpm/types'
import { sync as writeJson5File } from 'write-json5-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import writePkg from 'write-pkg'

export { Modules, Project }
export type ManifestFormat = 'JSON' | 'JSON5' | 'YAML'

// The testing folder should be outside of the project to avoid lookup in the project's node_modules
// Not using the OS temp directory due to issues on Windows CI.
const tmpPath = path.join(__dirname, `../../../../pnpm_tmp/${Math.random()}`)

let dirNumber = 0

export function tempDir () {
  dirNumber++
  const dirname = dirNumber.toString()
  const tmpDir = path.join(tmpPath, dirname)
  fs.mkdirSync(tmpDir, { recursive: true })

  process.chdir(tmpDir)

  return tmpDir
}

export function preparePackages (
  pkgs: Array<{ location: string, package: ProjectManifest } | ProjectManifest>,
  opts?: {
    manifestFormat?: ManifestFormat
    tempDir?: string
  }
) {
  const pkgTmpPath = opts?.tempDir ?? path.join(tempDir(), 'project')
  const manifestFormat = opts?.manifestFormat

  const dirname = path.dirname(pkgTmpPath)
  const result: { [name: string]: Project } = {}
  for (const aPkg of pkgs) {
    if (typeof aPkg['location'] === 'string') {
      result[aPkg['package']['name']] = prepare(aPkg['package'], {
        manifestFormat,
        tempDir: path.join(dirname, aPkg['location']),
      })
    } else {
      result[aPkg['name']] = prepare(aPkg as ProjectManifest, {
        manifestFormat,
        tempDir: path.join(dirname, aPkg['name']),
      })
    }
  }
  process.chdir('..')
  return result
}

export default function prepare (
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
