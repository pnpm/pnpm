import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import { isEmptyDirOrNothing } from '@pnpm/fs.is-empty-dir-or-nothing'
import { install } from '@pnpm/plugin-commands-installation'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { PnpmError } from '@pnpm/error'
import { readWantedLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import rimraf from '@zkochan/rimraf'
import renderHelp from 'render-help'
import { deployHook } from './deployHook'
import { logger, globalWarn } from '@pnpm/logger'
import { type Project, type ProjectId } from '@pnpm/types'
import normalizePath from 'normalize-path'
import { createDeployFiles } from './createDeployFiles'
import { deployCatalogHook } from './deployCatalogHook'

export const shorthands = install.shorthands

export function rcOptionsTypes (): Record<string, unknown> {
  return install.rcOptionsTypes()
}

export function cliOptionsTypes (): Record<string, unknown> {
  return install.cliOptionsTypes()
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
            description: 'Only `devDependencies` are installed regardless of the `NODE_ENV`',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: '`optionalDependencies` are not installed',
            name: '--no-optional',
          },
        ],
      },
      FILTERING,
    ],
  })
}

export type DeployOptions = Omit<install.InstallCommandOptions, 'useLockfile'>

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
    const warning = await deployFromSharedLockfile(opts, selectedProject, deployDir)
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
    depth: Infinity,
    hooks: {
      ...opts.hooks,
      readPackage: [
        ...(opts.hooks?.readPackage ?? []),
        deployHook,
        deployCatalogHook.bind(null, opts.catalogs ?? {}),
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
  const { lockfileDir, workspaceDir } = opts

  if (!lockfileDir) {
    return 'opts.lockfileDir is undefined. Falling back to installing without a lockfile.'
  }

  if (!workspaceDir) {
    return 'opts.workspaceDir is undefined. Falling back to installing without a lockfile.'
  }

  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })
  if (!lockfile) {
    return 'Shared lockfile not found. Falling back to installing without a lockfile.'
  }

  const projectId = normalizePath(path.relative(workspaceDir, selectedProject.rootDir)) as ProjectId

  const deployFiles = createDeployFiles({
    lockfile,
    lockfileDir,
    manifest: selectedProject.manifest,
    projectId,
    targetDir: deployDir,
  })

  await Promise.all([
    fs.promises.writeFile(
      path.join(deployDir, 'package.json'),
      JSON.stringify(deployFiles.manifest, undefined, 2) + '\n'
    ),
    writeWantedLockfile(deployDir, deployFiles.lockfile),
  ])

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
    hooks: {
      ...opts.hooks,
      readPackage: [
        ...(opts.hooks?.readPackage ?? []),
        deployHook,
        deployCatalogHook.bind(null, opts.catalogs ?? {}),
      ],
    },
    rawLocalConfig: {
      ...opts.rawLocalConfig,
      'frozen-lockfile': true,
    },
  })

  return undefined
}
