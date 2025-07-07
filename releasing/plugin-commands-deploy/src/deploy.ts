import fs from 'node:fs'
import path from 'node:path'
import pick from 'ramda/src/pick'
import { docsUrl } from '@pnpm/cli-utils'
import { type Config, types as configTypes } from '@pnpm/config'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import { isEmptyDirOrNothing } from '@pnpm/fs.is-empty-dir-or-nothing'
import { install } from '@pnpm/plugin-commands-installation'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { PnpmError } from '@pnpm/error'
import { getLockfileImporterId, readWantedLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import rimraf from '@zkochan/rimraf'
import renderHelp from 'render-help'
import { deployHook } from './deployHook'
import { logger, globalWarn } from '@pnpm/logger'
import { type Project } from '@pnpm/types'
import { createDeployFiles } from './createDeployFiles'

const FORCE_LEGACY_DEPLOY = 'force-legacy-deploy' satisfies keyof typeof configTypes

export const shorthands = {
  ...install.shorthands,
  legacy: [`--config.${FORCE_LEGACY_DEPLOY}=true`],
}

const DEPLOY_OWN_OPTIONS = pick([FORCE_LEGACY_DEPLOY], configTypes)

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...install.rcOptionsTypes(),
    ...DEPLOY_OWN_OPTIONS,
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...install.cliOptionsTypes(),
    ...DEPLOY_OWN_OPTIONS,
  }
}

export const commandNames = ['deploy']

export function help (): string {
  return renderHelp({
    description: 'Experimental! Deploy a package from a workspace',
    url: docsUrl('deploy'),
    usages: ['pnpm --filter=<deployed project name> deploy <target directory>'],
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: "Packages in `devDependencies` won't be installed",
            name: '--prod',
            shortAlias: '-P',
          },
          {
            description: 'Only `devDependencies` are installed',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: '`optionalDependencies` are not installed',
            name: '--no-optional',
          },
          {
            description: 'Force legacy deploy implementation',
            name: '--legacy',
          },
        ],
      },
      FILTERING,
    ],
  })
}

export type DeployOptions =
  & Omit<install.InstallCommandOptions, 'useLockfile'>
  & Pick<Config, 'forceLegacyDeploy'>

export async function handler (opts: DeployOptions, params: string[]): Promise<void> {
  if (!opts.workspaceDir) {
    throw new PnpmError('CANNOT_DEPLOY', 'A deploy is only possible from inside a workspace')
  }
  const selectedProjects = Object.values(opts.selectedProjectsGraph ?? {})
  if (selectedProjects.length === 0) {
    throw new PnpmError('NOTHING_TO_DEPLOY', 'No project was selected for deployment')
  }
  if (selectedProjects.length > 1) {
    throw new PnpmError('CANNOT_DEPLOY_MANY', 'Cannot deploy more than 1 project')
  }
  if (params.length !== 1) {
    throw new PnpmError('INVALID_DEPLOY_TARGET', 'This command requires one parameter')
  }
  const selectedProject = selectedProjects[0].package
  const deployDirParam = params[0]
  const deployDir = path.isAbsolute(deployDirParam) ? deployDirParam : path.join(opts.dir, deployDirParam)

  if (!isEmptyDirOrNothing(deployDir)) {
    if (!opts.force) {
      throw new PnpmError('DEPLOY_DIR_NOT_EMPTY', `Deploy path ${deployDir} is not empty`)
    }

    logger.warn({ message: 'using --force, deleting deploy path', prefix: deployDir })
  }

  await rimraf(deployDir)
  await fs.promises.mkdir(deployDir, { recursive: true })
  const includeOnlyPackageFiles = !opts.deployAllFiles
  await copyProject(selectedProject.rootDir, deployDir, { includeOnlyPackageFiles })

  if (opts.sharedWorkspaceLockfile) {
    const warning = opts.forceLegacyDeploy
      ? 'Shared workspace lockfile detected but configuration forces legacy deploy implementation.'
      : await deployFromSharedLockfile(opts, selectedProject, deployDir)
    if (warning) {
      globalWarn(warning)
    } else {
      return
    }
  }

  const deployedProject = opts.allProjects?.find(({ rootDir }) => rootDir === selectedProject.rootDir)
  if (deployedProject) {
    deployedProject.modulesDir = path.relative(selectedProject.rootDir, path.join(deployDir, 'node_modules'))
  }
  await install.handler({
    ...opts,
    confirmModulesPurge: false,
    // Deploy doesn't work with dedupePeerDependents=true currently as for deploy
    // we need to select a single project for install, while dedupePeerDependents
    // doesn't work with filters right now.
    // Related issue: https://github.com/pnpm/pnpm/issues/6858
    dedupePeerDependents: false,
    // If enabled, dedupe-injected-deps will symlink workspace packages in the
    // deployed dir to their original (non-deployed) directory in an attempt to
    // dedupe workspace packages that don't need to be injected. The deployed
    // dir shouldn't have symlinks to the original workspace. Disable
    // dedupe-injected-deps to always inject workspace packages since copying is
    // desirable.
    dedupeInjectedDeps: false,
    // Compute the wanted lockfile correctly by setting pruneLockfileImporters.
    // Since pnpm deploy only installs dependencies for a single selected
    // project, other projects in the "importers" lockfile section will be
    // empty when node-linker=hoisted.
    //
    // For example, when deploying project-1, project-2 may not be populated,
    // even if it has dependencies.
    //
    //   importers:
    //     project-1:
    //       dependencies:
    //         foo:
    //           specifier: ^1.0.0
    //           version: ^1.0.0
    //     project-2: {}
    //
    // Avoid including these empty importers in the in-memory wanted lockfile.
    // This is important when node-linker=hoisted to prevent project-2 from
    // being included in the hoisted install. If project-2 is errantly hoisted
    // to the root node_modules dir, downstream logic will fail to inject it to
    // the deploy directory. It's also just weird to include empty importers
    // that don't matter to the filtered lockfile generated for pnpm deploy.
    pruneLockfileImporters: true,
    depth: Infinity,
    hooks: {
      ...opts.hooks,
      readPackage: [
        ...(opts.hooks?.readPackage ?? []),
        deployHook,
      ],
    },
    frozenLockfile: false,
    preferFrozenLockfile: false,
    // Deploy doesn't work currently with hoisted node_modules.
    // TODO: make it work as we need to prefer packages from the lockfile during deployment.
    useLockfile: opts.nodeLinker !== 'hoisted',
    saveLockfile: false,
    virtualStoreDir: path.join(deployDir, 'node_modules/.pnpm'),
    modulesDir: path.relative(opts.workspaceDir, path.join(deployDir, 'node_modules')),
    rawLocalConfig: {
      ...opts.rawLocalConfig,
      // This is a workaround to prevent frozen install in CI envs.
      'frozen-lockfile': false,
    },
    includeOnlyPackageFiles,
  })
}

async function copyProject (src: string, dest: string, opts: { includeOnlyPackageFiles: boolean }): Promise<void> {
  const { filesIndex } = await fetchFromDir(src, opts)
  const importPkg = createIndexedPkgImporter('clone-or-copy')
  importPkg(dest, { filesMap: filesIndex, force: true, resolvedFrom: 'local-dir' })
}

async function deployFromSharedLockfile (
  opts: DeployOptions,
  selectedProject: Pick<Project, 'rootDir'> & {
    manifest: Pick<Project['manifest'], 'name' | 'version'>
  },
  deployDir: string
): Promise<string | undefined> {
  if (!opts.injectWorkspacePackages) {
    throw new PnpmError('DEPLOY_NONINJECTED_WORKSPACE', 'By default, starting from pnpm v10, we only deploy from workspaces that have "inject-workspace-packages=true" set', {
      hint: 'If you want to deploy without using injected dependencies, run "pnpm deploy" with the "--legacy" flag or set "force-legacy-deploy" to true',
    })
  }
  const {
    allProjects,
    lockfileDir,
    rootProjectManifest,
    rootProjectManifestDir,
    workspaceDir,
  } = opts

  // The following errors should not be possible. It is a programmer error if they are reached.
  if (!allProjects) throw new Error('opts.allProjects is undefined.')
  if (!lockfileDir) throw new Error('opts.lockfileDir is undefined.')
  if (!workspaceDir) throw new Error('opts.workspaceDir is undefined.')

  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })
  if (!lockfile) {
    return 'Shared lockfile not found. Falling back to installing without a lockfile.'
  }

  const projectId = getLockfileImporterId(lockfileDir, selectedProject.rootDir)

  const deployFiles = createDeployFiles({
    allProjects,
    deployDir,
    lockfile,
    lockfileDir,
    rootProjectManifest,
    selectedProjectManifest: selectedProject.manifest,
    projectId,
    rootProjectManifestDir,
  })

  await Promise.all([
    fs.promises.writeFile(
      path.join(deployDir, 'package.json'),
      JSON.stringify(deployFiles.manifest, undefined, 2) + '\n'
    ),
    writeWantedLockfile(deployDir, deployFiles.lockfile),
  ])

  try {
    await install.handler({
      ...opts,
      allProjects: undefined,
      allProjectsGraph: undefined,
      selectedProjectsGraph: undefined,
      rootProjectManifest: deployFiles.manifest,
      rootProjectManifestDir: deployDir,
      dir: deployDir,
      lockfileDir: deployDir,
      workspaceDir: undefined,
      virtualStoreDir: undefined,
      modulesDir: undefined,
      confirmModulesPurge: false,
      frozenLockfile: true,
      overrides: undefined, // the effects of the overrides should already be part of the package snapshots
      hooks: {
        ...opts.hooks,
        readPackage: [
          ...(opts.hooks?.readPackage ?? []),
          deployHook,
        ],
        calculatePnpmfileChecksum: undefined, // the effects of the pnpmfile should already be part of the package snapshots
      },
      rawLocalConfig: {
        ...opts.rawLocalConfig,
        'frozen-lockfile': true,
      },
    })
  } catch (error) {
    globalWarn(`Deployment with a shared lockfile has failed. If this is a bug, please report it at <https://github.com/pnpm/pnpm/issues>.
As a workaround, add the following to pnpm-workspace.yaml:

  forceLegacyDeploy: true`)
    throw error
  }

  return undefined
}
