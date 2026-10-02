import type { Project } from '@pnpm/types'

import { createRecursiveContext } from './recursive/context.js'
import { installEachProject } from './recursive/installEachProject.js'
import { installWithSharedLockfile } from './recursive/installWithSharedLockfile.js'
import type { CommandFullName, RecursiveOptions, RecursiveResult } from './recursive/options.js'

export type { CommandFullName, RecursiveOptions, RecursiveResult } from './recursive/options.js'
export {
  createMatcher,
  createUpdateMatching,
  expandUpdateSelectorsForMatching,
  failOnVersionsOfIndirectUpdateSpecs,
  makeIgnorePatterns,
  matchDependencies,
  parseUpdateParam,
  type UpdateDepsMatcher,
} from './recursive/updateSelectors.js'

const SHARED_LOCKFILE_COMMANDS: CommandFullName[] = ['add', 'install', 'remove', 'update', 'import']

export async function recursive (
  allProjects: Project[],
  params: string[],
  opts: RecursiveOptions,
  cmdFullName: CommandFullName
): Promise<RecursiveResult> {
  if (allProjects.length === 0) {
    // It might make sense to throw an exception in this case
    return { passed: false }
  }

  const selectedProjects = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)

  if (selectedProjects.length === 0) {
    return { passed: false }
  }
  const ctx = await createRecursiveContext({ allProjects, selectedProjects, params, opts, cmdFullName })
  // For a workspace with shared lockfile
  if (opts.lockfileDir && SHARED_LOCKFILE_COMMANDS.includes(cmdFullName)) {
    return installWithSharedLockfile(ctx)
  }
  return installEachProject(ctx)
}
