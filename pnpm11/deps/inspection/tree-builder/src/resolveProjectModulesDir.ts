import path from 'node:path'

import { importerModulesDir, isStrictlyInside } from '@pnpm/workspace.state'
import { pathAbsolute } from 'path-absolute'

import type { BaseTreeOpts } from './getTree.js'

export function resolveProjectModulesDir (
  projectPath: string,
  configuredModulesDir: string | undefined,
  opts: Pick<BaseTreeOpts, 'lockfileDir' | 'modulesDir' | 'nodeLinker'>
): string | undefined {
  const modulesDir = opts.nodeLinker === 'hoisted'
    ? importerModulesDir(projectPath, { lockfileDir: opts.lockfileDir, modulesDir: configuredModulesDir })
    : pathAbsolute(configuredModulesDir ?? 'node_modules', projectPath)
  const usesRootModulesDir = path.resolve(projectPath) === path.resolve(opts.lockfileDir) ||
    (opts.nodeLinker !== 'hoisted' && path.isAbsolute(configuredModulesDir ?? ''))
  if (usesRootModulesDir) return opts.modulesDir ?? modulesDir
  if (isStrictlyInside(modulesDir, projectPath) || isStrictlyInside(modulesDir, opts.lockfileDir)) {
    return modulesDir
  }
  return undefined
}
