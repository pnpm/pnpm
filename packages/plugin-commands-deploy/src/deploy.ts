import fs from 'fs'
import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import { install } from '@pnpm/plugin-commands-installation'
import PnpmError from '@pnpm/error'
import rimraf from '@zkochan/rimraf'
import renderHelp from 'render-help'

export const shorthands = install.shorthands

export function rcOptionsTypes () {
  return install.rcOptionsTypes()
}

export function cliOptionsTypes () {
  return install.cliOptionsTypes()
}

export const commandNames = ['deploy']

export function help () {
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
    ],
  })
}

export async function handler (
  opts: install.InstallCommandOptions,
  params: string[]
) {
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
  const deployDir = path.join(opts.workspaceDir, params[0])
  await rimraf(deployDir)
  await fs.promises.mkdir(deployDir, { recursive: true })
  await copyProject(deployedDir, deployDir)
  const readPackageHook = opts.hooks?.readPackage
  // eslint-disable-next-line
  const newReadPackageHook = readPackageHook ? (async (pkg: any, context: any) => deployHook(await readPackageHook(pkg, context))) : deployHook
  await install.handler({
    ...opts,
    depth: Infinity,
    hooks: {
      ...opts.hooks,
      readPackage: newReadPackageHook,
    },
    frozenLockfile: false,
    preferFrozenLockfile: false,
    saveLockfile: false,
    virtualStoreDir: path.join(deployDir, 'node_modules/.pnpm'),
    modulesDir: path.relative(deployedDir, path.join(deployDir, 'node_modules')),
    rawLocalConfig: {
      ...opts.rawLocalConfig,
      // This is a workaround to prevent frozen install in CI envs.
      'frozen-lockfile': false,
    },
  })
}

async function copyProject (src: string, dest: string) {
  const { filesIndex } = await fetchFromDir(src, { includeOnlyPackageFiles: true })
  const importPkg = createIndexedPkgImporter('clone-or-copy')
  await importPkg(dest, { filesMap: filesIndex, force: true, fromStore: true })
}

function deployHook (pkg: any) { // eslint-disable-line
  pkg.dependenciesMeta = pkg.dependenciesMeta || {}
  for (const [depName, depVersion] of Object.entries(pkg.dependencies ?? {})) {
    if ((depVersion as string).startsWith('workspace:')) {
      pkg.dependenciesMeta[depName] = {
        injected: true,
      }
    }
  }
  return pkg
}
