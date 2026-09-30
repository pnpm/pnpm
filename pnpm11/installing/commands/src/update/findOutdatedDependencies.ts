import { outdatedDepsOfProjects, type OutdatedPackage } from '@pnpm/deps.inspection.outdated'
import type { IncludedDependencies, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import type { UpdateCommandOptions } from './update.js'

export interface ProjectToUpdate {
  rootDir: ProjectRootDir
  manifest: ProjectManifest
}

export async function findOutdatedDependencies (
  projects: ProjectToUpdate[],
  selectors: string[],
  { include, opts }: { include: IncludedDependencies, opts: UpdateCommandOptions }
): Promise<OutdatedPackage[][]> {
  return outdatedDepsOfProjects(projects, selectors, {
    ...opts,
    compatible: opts.latest !== true,
    ignoreDependencies: opts.updateConfig?.ignoreDependencies,
    include,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  })
}
