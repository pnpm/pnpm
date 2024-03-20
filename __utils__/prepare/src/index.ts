import '@total-typescript/ts-reset'
import fs from 'node:fs'
import path from 'node:path'

import { assertProject } from '@pnpm/assert-project'
import type { AssertedProject, ProjectManifest } from '@pnpm/types'

import writePkg from 'write-pkg'
import uniqueString from 'unique-string'
import { sync as writeYamlFile } from 'write-yaml-file'
import { sync as writeJson5File } from 'write-json5-file'

export type ManifestFormat = 'JSON' | 'JSON5' | 'YAML'

// The testing folder should be outside of the project to avoid lookup in the project's node_modules
// Not using the OS temp directory due to issues on Windows CI.
const tmpPath = path.join(__dirname, `../../../../pnpm_tmp/${uniqueString()}`)

let dirNumber = 0

export function tempDir(chdir: boolean = true) {
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

export function preparePackages(
  pkgs: Array<LocationAndManifest | ProjectManifest>,
  opts?: {
    manifestFormat?: ManifestFormat
    tempDir?: string
  }
) {
  const pkgTmpPath = opts?.tempDir ?? path.join(tempDir(), 'project')
  const manifestFormat = opts?.manifestFormat

  const dirname = path.dirname(pkgTmpPath)
  const result: Record<string, AssertedProject> = {}

  const cwd = process.cwd()

  for (const aPkg of pkgs) {
    if ('location' in aPkg && typeof aPkg.location === 'string') {
      const name = aPkg.package.name
      if (typeof name !== 'undefined') {
        const prepared = prepare(
          aPkg.package,
          {
            manifestFormat,
            tempDir: path.join(dirname, aPkg.location),
          }
        )

        result[name] = prepared
      }
    } else if ('name' in aPkg && typeof aPkg.name === 'string') {
      result[aPkg.name] = prepare(
        aPkg,
        {
          manifestFormat,
          tempDir: path.join(dirname, aPkg.name),
        }
      )
    }
  }
  process.chdir(cwd)
  return result
}

export function prepare(
  manifest?: ProjectManifest | undefined,
  opts?: {
    manifestFormat?: ManifestFormat | undefined
    tempDir?: string | undefined
  } | undefined
): AssertedProject {
  const dir = opts?.tempDir ?? path.join(tempDir(), 'project')

  fs.mkdirSync(dir, { recursive: true })
  switch (opts?.manifestFormat ?? 'JSON') {
    case 'JSON': {
      // @ts-ignore
      writePkg.sync(dir, {
        name: 'project',
        version: '0.0.0',
        ...manifest,
      })
      break
    }
    case 'JSON5': {
      writeJson5File(path.join(dir, 'package.json5'), {
        name: 'project',
        version: '0.0.0',
        ...manifest,
      })
      break
    }
    case 'YAML': {
      writeYamlFile(path.join(dir, 'package.yaml'), {
        name: 'project',
        version: '0.0.0',
        ...manifest,
      })
      break
    }
  }
  process.chdir(dir)

  return assertProject(dir)
}

export function prepareEmpty() {
  const pkgTmpPath = path.join(tempDir(), 'project')

  fs.mkdirSync(pkgTmpPath, { recursive: true })
  process.chdir(pkgTmpPath)

  return assertProject(pkgTmpPath)
}
