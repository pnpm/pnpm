import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { lockfileToDepGraph, type DependenciesGraphNode } from '@pnpm/deps.graph-builder'
import { PnpmError } from '@pnpm/error'
import { filterLockfileByEngine } from '@pnpm/lockfile.filtering'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { type StoreController } from '@pnpm/store-controller-types'
import { type DepPath, type ProjectId, type Registries, type SupportedArchitectures } from '@pnpm/types'
import { symlinkAllModules } from '@pnpm/worker'

export interface StoreWarmupOptions {
  storeController: StoreController
  storeDir: string
  lockfileDir?: string
  dir: string
  registries: Registries
  force?: boolean
  engineStrict?: boolean
  nodeVersion?: string
  pnpmVersion?: string
  supportedArchitectures?: SupportedArchitectures
  virtualStoreDirMaxLength: number
}

export async function storeWarmup (opts: StoreWarmupOptions): Promise<void> {
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const wantedLockfile = await readWantedLockfile(lockfileDir, {
    ignoreIncompatible: false,
  })
  if (wantedLockfile == null) {
    throw new PnpmError('NO_LOCKFILE', `Cannot warm up the store without a ${WANTED_LOCKFILE} file`)
  }

  const globalVirtualStoreDir = path.join(opts.storeDir, 'links')
  const skipped = new Set<DepPath>()
  const include = { dependencies: true, devDependencies: true, optionalDependencies: true }
  const importerIds = Object.keys(wantedLockfile.importers) as ProjectId[]

  const { lockfile: filteredLockfile } = filterLockfileByEngine(wantedLockfile, {
    include,
    skipped,
    currentEngine: {
      nodeVersion: opts.nodeVersion ?? process.version,
      pnpmVersion: opts.pnpmVersion ?? '*',
    },
    engineStrict: opts.engineStrict ?? false,
    failOnMissingDependencies: false,
    includeIncompatiblePackages: opts.force ?? false,
    lockfileDir,
    supportedArchitectures: opts.supportedArchitectures,
  })

  const { graph } = await lockfileToDepGraph(
    filteredLockfile,
    null,
    {
      autoInstallPeers: false,
      enableGlobalVirtualStore: true,
      engineStrict: opts.engineStrict ?? false,
      force: opts.force ?? false,
      importerIds,
      include,
      ignoreScripts: true,
      ignoreLocalPackages: true,
      lockfileDir,
      nodeVersion: opts.nodeVersion ?? process.version,
      pnpmVersion: opts.pnpmVersion ?? '*',
      registries: opts.registries,
      sideEffectsCacheRead: false,
      skipped,
      storeController: opts.storeController,
      storeDir: opts.storeDir,
      globalVirtualStoreDir,
      virtualStoreDir: globalVirtualStoreDir,
      supportedArchitectures: opts.supportedArchitectures,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    }
  )

  const depNodes = Object.values(graph)

  // Import packages into GVS directories
  await Promise.all(depNodes.map(async (depNode: DependenciesGraphNode) => {
    if (!depNode.fetching) return
    let filesResponse
    try {
      filesResponse = (await depNode.fetching()).files
    } catch (err: unknown) {
      if (depNode.optional) return
      throw err
    }
    await opts.storeController.importPackage(depNode.dir, {
      filesResponse,
      force: opts.force ?? false,
      requiresBuild: false,
    })
  }))

  // Create internal node_modules symlinks within GVS dirs
  await symlinkAllModules({
    deps: depNodes.map((depNode) => ({
      children: depNode.children,
      modules: depNode.modules,
      name: depNode.name,
    })),
  })

  await opts.storeController.close()
}
