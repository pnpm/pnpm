import path from 'path'
import { PnpmError } from '@pnpm/error'
import { logger as createLogger } from '@pnpm/logger'
import { readModulesManifest } from '@pnpm/modules-yaml'
import normalizePath from 'normalize-path'
import { DirPatcher } from './DirPatcher'

interface LoggerPayloadBase {
  type: string
  msg: string
  opts: UpdateInjectedPackagesOptions
}

interface LoggerSkip extends LoggerPayloadBase {
  type: 'skip'
  reason: 'no-name' | 'no-injected-deps'
}

interface LoggerResync extends LoggerPayloadBase {
  type: 'resync'
  patcher: DirPatcher
}

type LoggerPayload = LoggerSkip | LoggerResync

const logger = createLogger<LoggerPayload>('update-injected-packages')

export interface UpdateInjectedPackagesOptions {
  pkgName: string | undefined
  pkgRootDir: string
  workspaceDir: string | undefined
}

export async function updateInjectedPackages (opts: UpdateInjectedPackagesOptions): Promise<void> {
  if (!opts.pkgName) {
    logger.debug({
      type: 'skip',
      reason: 'no-name',
      msg: `Skip updating ${opts.pkgRootDir} as an injected package because without name, it cannot be a dependency`,
      opts,
    })
    return
  }
  if (!opts.workspaceDir) {
    throw new PnpmError('NO_WORKSPACE_DIR', 'Cannot update injected packages without workspace dir')
  }
  const pkgRootDir = path.resolve(opts.workspaceDir, opts.pkgRootDir)
  const modulesDir = path.resolve(opts.workspaceDir, 'node_modules')
  const modules = await readModulesManifest(modulesDir)
  if (!modules?.injectedDeps) {
    logger.debug({
      type: 'skip',
      reason: 'no-injected-deps',
      msg: 'Skip updating injected packages because none were detected',
      opts,
    })
    return
  }
  const injectedDepKey = normalizePath(path.relative(opts.workspaceDir, pkgRootDir), true)
  const targetDirs: string[] | undefined = modules.injectedDeps[injectedDepKey]
  if (!targetDirs || targetDirs.length === 0) {
    logger.debug({
      type: 'skip',
      reason: 'no-injected-deps',
      msg: `There are no injected dependencies from ${opts.pkgRootDir}`,
      opts,
    })
    return
  }
  const patchers = await DirPatcher.fromMultipleTargets(
    pkgRootDir,
    targetDirs.map(targetDir => path.resolve(opts.workspaceDir!, targetDir))
  )
  await Promise.all(patchers.map(patcher => patcher.apply()))
}
