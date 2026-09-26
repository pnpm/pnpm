import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { assertProject, type Modules, type Project } from '@pnpm/assert-project'
import { tempDir } from '@pnpm/prepare-temp-dir'
import type { ProjectManifest } from '@pnpm/types'
import { writeJson5FileSync } from 'write-json5-file'
import { writePackageSync } from 'write-package'
import { writeYamlFileSync } from 'write-yaml-file'

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
      writePackageSync(dir, { name: 'project', version: '0.0.0', ...manifest } as any) // eslint-disable-line
      break
    case 'JSON5':
      writeJson5FileSync(path.join(dir, 'package.json5'), { name: 'project', version: '0.0.0', ...manifest } as any) // eslint-disable-line
      break
    case 'YAML':
      writeYamlFileSync(path.join(dir, 'package.yaml'), { name: 'project', version: '0.0.0', ...manifest } as any) // eslint-disable-line
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

/**
 * Kill a process started with `detached: true` together with everything it
 * started, whatever state a failed test left them in. Only a group that is
 * gone already is silently accepted.
 */
export function killProcessGroup (pid: number): void {
  try {
    process.kill(-pid, 'SIGKILL')
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ESRCH')) {
      throw err
    }
  }
}

/**
 * Whether the process `pid` is gone before `timeout` milliseconds pass. A
 * process that has just died still counts until it is reaped.
 */
export async function endsWithin (pid: number, timeout: number): Promise<boolean> {
  const deadline = Date.now() + timeout
  while (Date.now() < deadline) {
    if (!isRunning(pid)) return true
    await new Promise<void>((resolve) => setTimeout(resolve, 50)) // eslint-disable-line no-await-in-loop
  }
  return false
}

/**
 * A process the kernel no longer knows is gone, and so is one that has exited
 * and only waits to be reaped; one that refuses the probe is still there.
 */
function isRunning (pid: number): boolean {
  try {
    process.kill(pid, 0)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ESRCH') return false
    throw err
  }
  return !isZombie(pid)
}

/**
 * Whether `pid` has exited and waits for a parent to reap it, as an orphan
 * does until init takes it over.
 */
function isZombie (pid: number): boolean {
  if (process.platform === 'win32') return false
  const { error, stdout } = spawnSync('ps', ['-o', 'stat=', '-p', String(pid)], { encoding: 'utf8' })
  if (error) throw error
  return stdout.trimStart().startsWith('Z')
}
