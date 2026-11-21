import path from 'node:path'

import { existsInDir } from './existsInDir.js'

export function getNodeLoaderPaths (opts: {
  lockfileDir?: string
  workspaceDir?: string
  modulesDir?: string
  nodeExperimentalPackageMap?: boolean
}): {
  existsPnp: (dir: string) => string | undefined
  workspacePnpPath: string | undefined
  existsPackageMap: (dir: string) => string | undefined
  workspacePackageMapPath: string | undefined
} {
  const rootDir = opts.lockfileDir ?? opts.workspaceDir
  const existsPnp = (dir: string) => existsInDir('.pnp.cjs', opts.lockfileDir ?? dir)
  const existsPackageMap = (dir: string) => existsInDir(
    path.join(opts.modulesDir ?? 'node_modules', '.package-map.json'),
    opts.lockfileDir ?? dir
  )
  return {
    existsPnp,
    workspacePnpPath: rootDir ? existsPnp(rootDir) : undefined,
    existsPackageMap,
    workspacePackageMapPath: opts.nodeExperimentalPackageMap && rootDir ? existsPackageMap(rootDir) : undefined,
  }
}
