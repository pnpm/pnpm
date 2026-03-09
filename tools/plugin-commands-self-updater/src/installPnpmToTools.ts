import fs from 'fs'
import path from 'path'
import { getCurrentPackageName } from '@pnpm/cli-meta'
import { readConfigLockfile } from '@pnpm/config.deps-installer'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { mutateModulesInSingleProject } from '@pnpm/core'
import { writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { StoreController } from '@pnpm/package-store'
import { getToolDirPath } from '@pnpm/tools.path'
import type { DepPath, ProjectId, ProjectRootDir } from '@pnpm/types'
import { fastPathTemp as pathTemp } from 'path-temp'
import symlinkDir from 'symlink-dir'
import type { SelfUpdateCommandOptions } from './selfUpdate.js'

export interface InstallPnpmToToolsResult {
  binDir: string
  baseDir: string
  alreadyExisted: boolean
}

export interface InstallPnpmToToolsOptions extends SelfUpdateCommandOptions {
  storeController: StoreController
  storeDir: string
}

export async function installPnpmToTools (pnpmVersion: string, opts: InstallPnpmToToolsOptions): Promise<InstallPnpmToToolsResult> {
  const currentPkgName = getCurrentPackageName()
  const dir = getToolDirPath({
    pnpmHomeDir: opts.pnpmHomeDir,
    tool: {
      name: currentPkgName,
      version: pnpmVersion,
    },
  })

  const binDir = path.join(dir, 'bin')
  const alreadyExisted = fs.existsSync(binDir)
  if (alreadyExisted) {
    return {
      alreadyExisted,
      baseDir: dir,
      binDir,
    }
  }
  const stage = pathTemp(dir)
  fs.mkdirSync(stage, { recursive: true })
  fs.writeFileSync(path.join(stage, 'package.json'), JSON.stringify({
    dependencies: {
      [currentPkgName]: pnpmVersion,
    },
  }))

  try {
    const frozenLockfile = await tryWriteLockfileFromConfigLock(
      opts.rootProjectManifestDir,
      stage,
      currentPkgName,
      pnpmVersion
    )
    await mutateModulesInSingleProject(
      {
        manifest: {
          dependencies: {
            [currentPkgName]: pnpmVersion,
          },
        },
        mutation: 'install',
        rootDir: stage as ProjectRootDir,
        binsDir: path.join(stage, 'bin'),
      },
      {
        dir: stage,
        lockfileDir: stage,
        storeController: opts.storeController,
        storeDir: opts.storeDir,
        registries: opts.registries,
        ignoreScripts: true,
        nodeLinker: 'hoisted',
        frozenLockfile,
      }
    )
    if (currentPkgName === '@pnpm/exe') {
      linkExePlatformBinary(stage)
    }
    // We need the operation of installing pnpm to be atomic.
    // However, we cannot use a rename as that breaks the command shim created for pnpm.
    // Hence, we use a symlink.
    // In future we may switch back to rename if we will move Node.js out of the pnpm subdirectory.
    symlinkDir.sync(stage, dir)
  } catch (err: unknown) {
    try {
      fs.rmSync(stage, { recursive: true, force: true })
    } catch {} // eslint-disable-line:no-empty
    throw err
  }
  return {
    alreadyExisted,
    baseDir: dir,
    binDir,
  }
}

/**
 * Tries to create a pnpm-lock.yaml from the packageManager section of pnpm-config-lock.yaml.
 * Returns true if a frozen lockfile was written, false otherwise.
 */
async function tryWriteLockfileFromConfigLock (
  rootProjectManifestDir: string,
  stageDir: string,
  pkgName: string,
  version: string
): Promise<boolean> {
  const configLockfile = await readConfigLockfile(rootProjectManifestDir)
  if (!configLockfile?.packageManager) return false

  const pkgKey = `${pkgName}@${version}` as DepPath
  if (!configLockfile.packageManager[pkgKey]) return false

  // Build a lockfile from the stored integrities
  const packages: Record<string, { resolution: { integrity: string } }> = {}
  for (const [depPath, info] of Object.entries(configLockfile.packageManager)) {
    packages[depPath] = {
      resolution: { integrity: info.resolution.integrity },
    }
  }

  // Find dependencies of the main package to build snapshots
  // The packageManager section stores flat integrities — we write them as packages
  // and let the lockfile be used for integrity verification
  const specifiers: Record<string, string> = {}
  specifiers[pkgName] = version

  const dependencies: Record<string, string> = {}
  dependencies[pkgName] = version

  await writeWantedLockfile(stageDir, {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      ['.' as ProjectId]: {
        specifiers,
        dependencies,
      },
    },
    packages: packages as any, // eslint-disable-line @typescript-eslint/no-explicit-any
  })

  return true
}

// This replicates the logic from @pnpm/exe's setup.js (pnpm/artifacts/exe/setup.js).
// We can't run setup.js via require() or import() because:
// - require() fails when setup.js is ESM (pnpm v11+)
// - import() is intercepted by pkg's virtual filesystem in standalone executables
// So we inline the logic: find the platform-specific binary and hard-link it
// into the @pnpm/exe package directory.
function linkExePlatformBinary (stageDir: string): void {
  const platform = process.platform === 'win32'
    ? 'win'
    : process.platform === 'darwin'
      ? 'macos'
      : process.platform
  const arch = platform === 'win' && process.arch === 'ia32' ? 'x86' : process.arch
  const executable = platform === 'win' ? 'pnpm.exe' : 'pnpm'
  const platformPkgDir = path.join(stageDir, 'node_modules', '@pnpm', `${platform}-${arch}`)
  const src = path.join(platformPkgDir, executable)
  if (!fs.existsSync(src)) return
  const exePkgDir = path.join(stageDir, 'node_modules', '@pnpm', 'exe')
  const dest = path.join(exePkgDir, executable)
  try {
    fs.unlinkSync(dest)
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
  }
  fs.linkSync(src, dest)
  fs.chmodSync(dest, 0o755)
  if (platform === 'win') {
    const exePkgJsonPath = path.join(exePkgDir, 'package.json')
    const exePkg = JSON.parse(fs.readFileSync(exePkgJsonPath, 'utf8'))
    fs.writeFileSync(path.join(exePkgDir, 'pnpm'), 'This file intentionally left blank')
    exePkg.bin.pnpm = 'pnpm.exe'
    fs.writeFileSync(exePkgJsonPath, JSON.stringify(exePkg, null, 2))
  }
}
