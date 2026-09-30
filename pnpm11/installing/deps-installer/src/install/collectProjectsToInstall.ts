import { createOverriddenDependencyMatcher, createReadPackageHook } from '@pnpm/hooks.read-package-hook'

import { installCase } from './installCase.js'
import { installSome } from './installSome.js'
import type { ImporterToUpdate, InstallDepsMutation, InstallSomeDepsMutation, MutatedProject, MutationRun } from './mutationTypes.js'
import { type InstallSomeProject, memoizePreferredSpecs, type ProjectCollector } from './projectCollector.js'

/**
 * Turns every mutated project into the importer the resolution installs,
 * with the dependencies it wants.
 */
export async function collectProjectsToInstall (run: MutationRun): Promise<ImporterToUpdate[]> {
  const { ctx, opts, projects } = run
  const collector: ProjectCollector = {
    run,
    projectsToInstall: [],
    installedProjectIds: new Set<string>(projects.map((project) => ctx.projects[project.rootDir].id)),
    overriddenDependencyMatcherFor: createOverriddenDependencyMatcher(opts.parsedOverrides, opts.lockfileDir),
    applyOverrides: createReadPackageHook({
      ignoreCompatibilityDb: true,
      lockfileDir: opts.lockfileDir,
      overrides: opts.parsedOverrides,
    }),
    getPreferredSpecs: memoizePreferredSpecs(ctx),
  }

  // TODO: make it concurrent
  /* eslint-disable no-await-in-loop -- projects share the lazily computed preferredSpecs */
  for (const project of projects) {
    await collectProjectToInstall(collector, project)
  }
  /* eslint-enable no-await-in-loop */
  return collector.projectsToInstall
}

async function collectProjectToInstall (collector: ProjectCollector, project: MutatedProject): Promise<void> {
  const projectOpts = {
    ...project,
    ...collector.run.ctx.projects[project.rootDir],
  }
  switch (project.mutation) {
    case 'uninstallSome':
      collector.projectsToInstall.push({
        pruneDirectDependencies: false,
        ...projectOpts,
        removePackages: project.dependencyNames,
        updatePackageManifest: true,
        wantedDependencies: [],
      })
      break
    case 'install': {
      await installCase(collector, {
        ...projectOpts,
        updatePackageManifest: (projectOpts as InstallDepsMutation).updatePackageManifest ?? (projectOpts as InstallDepsMutation).update!,
      })
      break
    }
    case 'installSome': {
      await installSome(collector, {
        ...projectOpts as InstallSomeProject,
        updatePackageManifest: (projectOpts as InstallSomeDepsMutation).updatePackageManifest !== false,
      })
      break
    }
  }
}
