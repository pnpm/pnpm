import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import { isEmptyDirOrNothing } from '@pnpm/fs.is-empty-dir-or-nothing'
import { install } from '@pnpm/plugin-commands-installation'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { PnpmError } from '@pnpm/error'
import rimraf from '@zkochan/rimraf'
import renderHelp from 'render-help'
import { deployHook } from './deployHook'
import { logger } from '@pnpm/logger'
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

export async function handler (
  opts: Omit<install.InstallCommandOptions, 'useLockfile'>,
  params: string[]
): Promise<void> {
  if (!opts.workspaceDir) {
    throw new PnpmError('CANNOT_DEPLOY', 'A deploy is only possible from inside a workspace')
  }
  const selectedDirs = Object.keys(opts.selectedProjectsGraph ?? {})
  if (selectedDirs.length === 0) {
    throw new PnpmError('NOTHING_TO_DEPLOY', 'No project was selected for deployment')
  }
  if (selectedDirs.length > 1) {
    throw new PnpmError('CANNOT_DEPLOY_MANY', 'Cannot deploy more than 1 project')
  }
  if (params.length !== 1) {
    throw new PnpmError('INVALID_DEPLOY_TARGET', 'This command requires one parameter')
  }
  const deployedDir = selectedDirs[0]
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
  await copyProject(deployedDir, deployDir, { includeOnlyPackageFiles })
  const deployedProject = opts.allProjects?.find(({ rootDir }) => rootDir === deployedDir)
  if (deployedProject) {
    deployedProject.modulesDir = path.relative(deployedDir, path.join(deployDir, 'node_modules'))
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
