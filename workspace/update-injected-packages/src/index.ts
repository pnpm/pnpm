import path from 'path'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { PnpmError } from '@pnpm/error'
import { type ImportOptions, createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import { globalInfo } from '@pnpm/logger'
import { readModulesManifest } from '@pnpm/modules-yaml'
import normalizePath from 'normalize-path'

export interface UpdateInjectedPackagesOptions {
  pkgName: string | undefined
  pkgRootDir: string
  // modulesDir: string | undefined
  workspaceDir: string | undefined
}

export async function updateInjectedPackages (opts: UpdateInjectedPackagesOptions): Promise<void> {
  if (!opts.pkgName) {
    globalInfo(`Skip updating ${opts.pkgRootDir} as an injected package because without name, it cannot be a dependency`)
    return
  }
  if (!opts.workspaceDir) {
    throw new PnpmError('NO_WORKSPACE_DIR', 'Cannot update injected packages without workspace dir')
  }
  const pkgRootDir = path.resolve(opts.workspaceDir, opts.pkgRootDir)
  const modulesDir = /* opts.modulesDir ?? */ path.resolve(opts.workspaceDir, 'node_modules')
  const modules = await readModulesManifest(modulesDir)
  if (!modules?.injectedDeps) {
    globalInfo('Skip updating injected packages because none were detected')
    return
  }
  const injectedDepKey = normalizePath(path.relative(opts.workspaceDir, pkgRootDir), true)
  const targetDirs: string[] | undefined = modules.injectedDeps[injectedDepKey]
  if (!targetDirs || targetDirs.length === 0) {
    globalInfo(`There are no injected dependency from ${opts.pkgRootDir}`)
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
    globalInfo(`Importing ${targetDirRealPath} from ${pkgRootDir}`)
    importPackage(targetDirRealPath, importOptions)
  }
}
