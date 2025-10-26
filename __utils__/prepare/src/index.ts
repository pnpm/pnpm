import fs from 'fs'
import path from 'path'
import { assertProject, type Modules, type Project } from '@pnpm/assert-project'
import { type ProjectManifest } from '@pnpm/types'
import { tempDir } from '@pnpm/prepare-temp-dir'
import { sync as writeJson5File } from 'write-json5-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import writePkg from 'write-pkg'

export type { Modules, Project }
export type ManifestFormat = 'JSON' | 'JSON5' | 'YAML'
export { tempDir }

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
): Record<string, Project> {
  const pkgTmpPath = opts?.tempDir ?? path.join(tempDir(), 'project')
  const manifestFormat = opts?.manifestFormat

  const dirname = path.dirname(pkgTmpPath)
  const result: Record<string, Project> = {}
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
): Project {
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

export function prepareEmpty (): Project {
  const pkgTmpPath = path.join(tempDir(), 'project')

  fs.mkdirSync(pkgTmpPath, { recursive: true })
  process.chdir(pkgTmpPath)

  return assertProject(pkgTmpPath)
}
