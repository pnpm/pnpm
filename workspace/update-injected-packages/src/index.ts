import path from 'path'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { PnpmError } from '@pnpm/error'
import { type ImportOptions, createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import { logger as createLogger } from '@pnpm/logger'
import { readModulesManifest } from '@pnpm/modules-yaml'
import normalizePath from 'normalize-path'

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
  sourceDir: string
  targetDir: string
}

type LoggerPayload = LoggerSkip | LoggerResync

const logger = createLogger<LoggerPayload>('update-injected-packages')

export interface UpdateInjectedPackagesOptions {
  pkgName: string | undefined
  pkgRootDir: string
  // modulesDir: string | undefined
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
  const modulesDir = /* opts.modulesDir ?? */ path.resolve(opts.workspaceDir, 'node_modules')
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
  const { filesIndex } = await fetchFromDir(pkgRootDir, {})
  const importOptions: ImportOptions = {
    filesMap: filesIndex,
    force: true,
    resolvedFrom: 'local-dir',
  }
  const importPackage = createIndexedPkgImporter('hardlink')
  for (const targetDir of targetDirs) {
    const targetDirRealPath = path.resolve(opts.workspaceDir, targetDir)
    logger.debug({
      type: 'resync',
      msg: `Importing ${targetDirRealPath} from ${pkgRootDir}`,
      sourceDir: pkgRootDir,
      targetDir: targetDirRealPath,
      opts,
    })
    importPackage(targetDirRealPath, importOptions)
  }
}
