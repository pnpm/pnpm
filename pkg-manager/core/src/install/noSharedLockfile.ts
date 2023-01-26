import {
  PeerDependencyRules,
  PnpmOptions,
} from '@pnpm/types'
import { Hooks, StrictInstallOptions } from './extendInstallOptions'
import { ProjectOptions } from '@pnpm/get-context'
import { ReporterFunction } from '../types'
import { MutatedProject, mutateModules, MutateModulesOptions, UpdatedProject } from '.'
import pathAbsolute from 'path-absolute'
import path from 'path'

/// FIXME: temporary solutions
interface PartialOptions {
  extraEnv?: Record<string, string>
  linkWorkspacePackagesDepth?: number
  reporter?: ReporterFunction
  forcePublicHoistPattern?: boolean
  modulesDir?: string
  peerDependencyRules?: PeerDependencyRules
  preferSymlinkedExecutables?: boolean
  global?: boolean
  forceHoistPattern?: boolean
  forceShamefullyHoist?: boolean
  dir?: string
}

type ProjectInfo = PnpmOptions & ProjectOptions & { hooks: Hooks }
type SplitLockfileInstallOptions = Omit<StrictInstallOptions,
keyof PnpmOptions | keyof PartialOptions | 'hooks' | 'lockfileDir' | 'allProjects'>
& PartialOptions & {
  allProjects: ProjectInfo[]
  workspaceDir: string
}

type InstallOptions = Partial<SplitLockfileInstallOptions> & Pick<SplitLockfileInstallOptions, 'storeDir' | 'storeController' | 'workspaceDir'>

export async function mutateProjectsIndependently (
  projects: MutatedProject[],
  maybeOpts: InstallOptions
): Promise<UpdatedProject[]> {
  if (maybeOpts.allProjects) {
    const promises = maybeOpts.allProjects.map(project => task(projects, project, maybeOpts))
    const results = await Promise.all(promises)
    return results.flat()
  } else {
    return []
  }
}

async function task (mutatedImports: MutatedProject[], project: ProjectInfo, splitLockFileOpts: InstallOptions) {
  const modulesDir = splitLockFileOpts.modulesDir ?? 'node_modules'
  const virtualStoreDir = pathAbsolute(path.resolve(modulesDir, '.pnpm'), splitLockFileOpts.workspaceDir)
  const opts: MutateModulesOptions = {
    ...splitLockFileOpts,
    ...project,
    virtualStoreDir,
    lockfileDir: project.rootDir,
    allProjects: [project],
    pruneVirtualStore: false,
  }
  const mutatedImport = mutatedImports.find((p) => p.rootDir === project.rootDir)
  if (!mutatedImport) {
    return []
  } else {
    return mutateModules([mutatedImport], opts)
  }
}
