import assert from 'node:assert'

import { type Config, createProjectModulesDirResolver } from '@pnpm/config.reader'
import {
  type DependentsTree,
  getDependentsTrees,
  mergeDependentsTrees,
  renderDependents,
  renderJson,
  whyForPackages,
  type WhyOptions,
} from '@pnpm/deps.inspection.list'
import { isError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import type { IncludedDependencies, Project } from '@pnpm/types'

import { determineReportAs } from './common.js'
import { loadProjectHierarchies, render } from './list.js'

export async function listRecursive (
  pkgs: Project[],
  params: string[],
  opts: Pick<Config, 'lockfileDir' | 'virtualStoreDirMaxLength' | 'modulesDir' | 'packageConfigs'> & {
    depth?: number
    include: IncludedDependencies
    long?: boolean
    json?: boolean
    parseable?: boolean
    lockfileDir?: string
    checkWantedLockfileOnly?: boolean
    onlyProjects?: boolean
    findBy?: string[]
    workspaceProjectDirs?: string[]
    workspaceProjectPublishDirs?: Record<string, string>
  }
): Promise<string> {
  const depth = opts.depth ?? 0
  // --only-projects lists the selected projects themselves, so each is printed
  // even when it links no other project. A search prints only the matches.
  const alwaysPrintRootPackage = depth === -1 ||
    (opts.onlyProjects === true && params.length === 0 && !opts.findBy?.length)
  if (opts.lockfileDir) {
    return render(pkgs.map((pkg) => pkg.rootDir), params, {
      ...opts,
      alwaysPrintRootPackage,
      lockfileDir: opts.lockfileDir,
    })
  }
  const resolveModulesDir = createProjectModulesDirResolver(opts)
  const projectOpts = ({ rootDir, manifest }: Project) => ({
    ...opts,
    modulesDir: resolveModulesDir(manifest.name),
    lockfileDir: rootDir,
  })
  if (determineReportAs(opts) === 'json') {
    const projects = await Promise.all(pkgs.map((pkg) =>
      withProjectError(pkg.rootDir, () => loadProjectHierarchies([pkg.rootDir], params, projectOpts(pkg)))
    ))
    return renderJson(projects.flat(), { depth, long: opts.long ?? false, search: params.length > 0 })
  }
  const outputs = (await Promise.all(pkgs.map((pkg) =>
    withProjectError(pkg.rootDir, () => render([pkg.rootDir], params, {
      ...projectOpts(pkg),
      alwaysPrintRootPackage,
    }))
  ))).filter(Boolean)
  if (outputs.length === 0) return ''

  const joiner = typeof depth === 'number' && depth > -1 ? '\n\n' : '\n'
  return outputs.join(joiner)
}

export async function whyRecursive (
  pkgs: Project[],
  params: string[],
  opts: Omit<WhyOptions, 'lockfileDir'> & Pick<Config, 'lockfileDir' | 'packageConfigs'>
): Promise<string> {
  if (opts.lockfileDir) {
    return whyForPackages(params, pkgs.map((pkg) => pkg.rootDir), {
      ...opts,
      lockfileDir: opts.lockfileDir,
    })
  }
  const resolveModulesDir = createProjectModulesDirResolver(opts)
  const projectTrees = (await Promise.all(pkgs.map((pkg) =>
    withProjectError(pkg.rootDir, () => getDependentsTrees(params, [pkg.rootDir], {
      ...opts,
      lockfileDir: pkg.rootDir,
      modulesDir: resolveModulesDir(pkg.manifest.name),
    }))
  ))).filter((trees): trees is DependentsTree[] => trees != null)
  if (projectTrees.length === 0) return ''

  const trees = mergeDependentsTrees(projectTrees.flat())
  return renderDependents(trees, opts)
}

async function withProjectError<Result> (rootDir: string, action: () => Promise<Result>): Promise<Result> {
  try {
    return await action()
  } catch (err: unknown) {
    assert(isError(err))
    const errWithPrefix = Object.assign(err, { prefix: rootDir })
    logger.info(errWithPrefix)
    throw errWithPrefix
  }
}
