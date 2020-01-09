import {
  createLatestSpecs,
  docsUrl,
  getPinnedVersion,
  getSaveType,
  readProjectManifestOnly,
  tryReadProjectManifest,
  updateToLatestSpecsFromManifest,
} from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { filterPkgsBySelectorObjects } from '@pnpm/filter-workspace-packages'
import findWorkspacePackages, { arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'
import { rebuild } from '@pnpm/plugin-commands-rebuild/lib/implementation'
import { requireHooks } from '@pnpm/pnpmfile'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { oneLine } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import {
  install,
  mutateModules,
} from 'supi'
import recursive from './recursive'
import { createWorkspaceSpecs, updateToWorkspacePackagesFromManifest } from './updateWorkspaceDependencies'

const OVERWRITE_UPDATE_OPTIONS = {
  allowNew: true,
  update: false,
}

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return R.pick([
    'child-concurrency',
    'dev',
    'engine-strict',
    'frozen-lockfile',
    'force',
    'global-dir',
    'global-pnpmfile',
    'global',
    'hoist',
    'hoist-pattern',
    'ignore-pnpmfile',
    'ignore-scripts',
    'independent-leaves',
    'link-workspace-packages',
    'lock',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'package-import-method',
    'pnpmfile',
    'prefer-frozen-lockfile',
    'prefer-offline',
    'production',
    'recursive',
    'registry',
    'reporter',
    'resolution-strategy',
    'shamefully-flatten',
    'shamefully-hoist',
    'shared-workspace-lockfile',
    'side-effects-cache-readonly',
    'side-effects-cache',
    'store',
    'store-dir',
    'strict-peer-dependencies',
    'offline',
    'only',
    'optional',
    'use-running-store-server',
    'use-store-server',
    'verify-store-integrity',
    'virtual-store-dir',
  ], allTypes)
}

export const commandNames = ['install', 'i']

export function help () {
  return renderHelp({
    aliases: ['i'],
    description: oneLine`Installs all dependencies of the project in the current working directory.
      When executed inside a workspace, installs all dependencies of all projects.`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: oneLine`
              Run installation recursively in every package found in subdirectories.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            name: '--recursive',
            shortAlias: '-r',
          },
          OPTIONS.ignoreScripts,
          OPTIONS.offline,
          OPTIONS.preferOffline,
          OPTIONS.globalDir,
          {
            description: "Packages in \`devDependencies\` won't be installed",
            name: '--production',
          },
          {
            description: 'Only \`devDependencies\` are installed regardless of the \`NODE_ENV\`',
            name: '--dev',
          },
          {
            description: '`optionalDependencies` are not installed',
            name: '--no-optional',
          },
          {
            description: `Don't read or generate a \`${WANTED_LOCKFILE}\` file`,
            name: '--no-lockfile',
          },
          {
            description: `Dependencies are not downloaded. Only \`${WANTED_LOCKFILE}\` is updated`,
            name: '--lockfile-only',
          },
          {
            description: "Don't generate a lockfile and fail if an update is needed",
            name: '--frozen-lockfile',
          },
          {
            description: `If the available \`${WANTED_LOCKFILE}\` satisfies the \`package.json\` then perform a headless installation`,
            name: '--prefer-frozen-lockfile',
          },
          {
            description: `The directory in which the ${WANTED_LOCKFILE} of the package will be created. Several projects may share a single lockfile`,
            name: '--lockfile-dir <dir>',
          },
          {
            description: 'Dependencies inside node_modules have access only to their listed dependencies',
            name: '--no-hoist',
          },
          {
            description: 'The subdeps will be hoisted into the root node_modules. Your code will have access to them',
            name: '--shamefully-hoist',
          },
          {
            description: oneLine`
              Hoist all dependencies matching the pattern to \`node_modules/.pnpm/node_modules\`.
              The default pattern is * and matches everything. Hoisted packages can be required
              by any dependencies, so it is an emulation of a flat node_modules`,
            name: '--hoist-pattern <pattern>',
          },
          OPTIONS.storeDir,
          OPTIONS.virtualStoreDir,
          {
            description: 'Maximum number of concurrent network requests',
            name: '--network-concurrency <number>',
          },
          {
            description: 'Controls the number of child processes run parallelly to build node modules',
            name: '--child-concurrency <number>',
          },
          {
            description: 'Disable pnpm hooks defined in pnpmfile.js',
            name: '--ignore-pnpmfile',
          },
          {
            description: 'Symlinks leaf dependencies directly from the global store',
            name: '--independent-leaves',
          },
          {
            description: "If false, doesn't check whether packages in the store were mutated",
            name: '--[no-]verify-store-integrity',
          },
          {
            name: '--[no-]lock',
          },
          {
            description: 'Fail on missing or invalid peer dependencies',
            name: '--strict-peer-dependencies',
          },
          {
            description: 'Starts a store server in the background. The store server will keep running after installation is done. To stop the store server, run \`pnpm server stop\`',
            name: '--use-store-server',
          },
          {
            description: 'Only allows installation with a store server. If no store server is running, installation will fail',
            name: '--use-running-store-server',
          },
          {
            description: 'Clones/hardlinks or copies packages. The selected method depends from the file system',
            name: '--package-import-method auto',
          },
          {
            description: 'Hardlink packages from the store',
            name: '--package-import-method hardlink',
          },
          {
            description: 'Copy packages from the store',
            name: '--package-import-method copy',
          },
          {
            description: 'Clone (aka copy-on-write) packages from the store',
            name: '--package-import-method clone',
          },
          {
            description: 'The default resolution strategy. Speed is preferred over deduplication',
            name: '--resolution-strategy fast',
          },
          {
            description: 'Already installed dependencies are preferred even if newer versions satisfy a range',
            name: '--resolution-strategy fewer-dependencies',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
      {
        title: 'Output',

        list: [
          {
            description: 'No output is logged to the console, except fatal errors',
            name: '--silent, --reporter silent',
            shortAlias: '-s',
          },
          {
            description: 'The default reporter when the stdout is TTY',
            name: '--reporter default',
          },
          {
            description: 'The output is always appended to the end. No cursor manipulations are performed',
            name: '--reporter append-only',
          },
          {
            description: 'The most verbose reporter. Prints all logs in ndjson format',
            name: '--reporter ndjson',
          },
        ],
      },
      FILTERING,
      {
        title: 'Experimental options',

        list: [
          {
            description: 'Use or cache the results of (pre/post)install hooks',
            name: '--side-effects-cache',
          },
          {
            description: 'Only use the side effects cache if present, do not create it for new packages',
            name: '--side-effects-cache-readonly',
          },
        ],
      },
    ],
    url: docsUrl('install'),
    usages: ['pnpm install [options]'],
  })
}

export type InstallCommandOptions = Pick<Config,
  'allProjects' |
  'bail' |
  'bin' |
  'cliOptions' |
  'dev' |
  'engineStrict' |
  'globalPnpmfile' |
  'ignorePnpmfile' |
  'ignoreScripts' |
  'linkWorkspacePackages' |
  'lockfileDir' |
  'pnpmfile' |
  'production' |
  'rawLocalConfig' |
  'registries' |
  'save' |
  'saveDev' |
  'saveExact' |
  'saveOptional' |
  'savePeer' |
  'savePrefix' |
  'saveProd' |
  'saveWorkspaceProtocol' |
  'selectedProjectsGraph' |
  'sideEffectsCache' |
  'sideEffectsCacheReadonly' |
  'sort' |
  'sharedWorkspaceLockfile' |
  'optional' |
  'workspaceConcurrency' |
  'workspaceDir'
> & CreateStoreControllerOptions & {
  argv: {
    original: string[],
  },
  allowNew?: boolean,
  latest?: boolean,
  update?: boolean,
  useBetaCli?: boolean,
  recursive?: boolean,
  workspace?: boolean,
}

export async function handler (
  input: string[],
  opts: InstallCommandOptions,
) {
  if (opts.workspace) {
    if (opts.latest) {
      throw new PnpmError('BAD_OPTIONS', 'Cannot use --latest with --workspace simultaneously')
    }
    if (!opts.workspaceDir) {
      throw new PnpmError('WORKSPACE_OPTION_OUTSIDE_WORKSPACE', '--workspace can only be used inside a workspace')
    }
    if (!opts.linkWorkspacePackages && !opts.saveWorkspaceProtocol) {
      if (opts.rawLocalConfig['save-workspace-protocol'] === false) {
        throw new PnpmError('BAD_OPTIONS', oneLine`This workspace has link-workspace-packages turned off,
          so dependencies are linked from the workspace only when the workspace protocol is used.
          Either set link-workspace-packages to true or don't use the --no-save-workspace-protocol option
          when running add/update with the --workspace option`)
      } else {
        opts.saveWorkspaceProtocol = true
      }
    }
    opts['preserveWorkspaceProtocol'] = !opts.linkWorkspacePackages
  }
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  if (opts.recursive && opts.allProjects && opts.selectedProjectsGraph && opts.workspaceDir) {
    await recursive(opts.allProjects,
      input,
      {
        ...opts,
        include,
        selectedProjectsGraph: opts.selectedProjectsGraph!,
        workspaceDir: opts.workspaceDir!,
      },
      opts.update ? 'update' : (input.length === 0 ? 'install' : 'add'),
    )
    return
  }
  // `pnpm install ""` is going to be just `pnpm install`
  input = input.filter(Boolean)

  const dir = opts.dir || process.cwd()
  let allProjects = opts.allProjects
  let workspacePackages = undefined

  if (opts.workspaceDir) {
    allProjects = allProjects ?? await findWorkspacePackages(opts.workspaceDir, opts)
    workspacePackages = arrayOfWorkspacePackagesToMap(allProjects)
  }

  const store = await createOrConnectStoreController(opts)
  const installOpts = {
    ...opts,
    // In case installation is done in a multi-package repository
    // The dependencies should be built first,
    // so ignoring scripts for now
    ignoreScripts: !!workspacePackages || opts.ignoreScripts,
    include,
    sideEffectsCacheRead: opts.sideEffectsCache || opts.sideEffectsCacheReadonly,
    sideEffectsCacheWrite: opts.sideEffectsCache,
    storeController: store.ctrl,
    storeDir: store.dir,
    workspacePackages,

    forceHoistPattern: typeof opts.rawLocalConfig['hoist-pattern'] !== 'undefined' || typeof opts.rawLocalConfig['hoist'] !== 'undefined',
    forceIndependentLeaves: typeof opts.rawLocalConfig['independent-leaves'] !== 'undefined',
    forceShamefullyHoist: typeof opts.rawLocalConfig['shamefully-hoist'] !== 'undefined',
  }
  if (!opts.ignorePnpmfile) {
    installOpts['hooks'] = requireHooks(opts.lockfileDir || dir, opts)
  }

  let { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.dir, opts)
  if (manifest === null) {
    if (opts.update) {
      throw new PnpmError('NO_IMPORTER_MANIFEST', 'No package.json found')
    }
    manifest = {}
  }

  if (opts.update && opts.latest) {
    if (!input || !input.length) {
      input = updateToLatestSpecsFromManifest(manifest, include)
    } else {
      input = createLatestSpecs(input, manifest)
    }
    delete installOpts.include
  }
  if (opts.workspace) {
    if (!input || !input.length) {
      input = updateToWorkspacePackagesFromManifest(manifest, include, workspacePackages!)
    } else {
      input = createWorkspaceSpecs(input, workspacePackages!)
    }
  }
  if (!input || !input.length) {
    const updatedManifest = await install(manifest, installOpts)
    if (opts.update === true && opts.save !== false) {
      await writeProjectManifest(updatedManifest)
    }
  } else {
    const [updatedImporter] = await mutateModules([
      {
        allowNew: opts.allowNew,
        binsDir: installOpts.bin,
        dependencySelectors: input,
        manifest,
        mutation: 'installSome',
        peer: opts.savePeer,
        pinnedVersion: getPinnedVersion(opts),
        rootDir: installOpts.dir,
        targetDependenciesField: getSaveType(installOpts),
      },
    ], installOpts)
    if (opts.save !== false) {
      await writeProjectManifest(updatedImporter.manifest)
    }
  }

  if (opts.linkWorkspacePackages && opts.workspaceDir) {
    allProjects = allProjects ?? await findWorkspacePackages(opts.workspaceDir, opts)
    const selectedProjectsGraph = await filterPkgsBySelectorObjects(allProjects, [
      {
        excludeSelf: true,
        includeDependencies: true,
        parentDir: dir,
      },
    ], {
      workspaceDir: opts.workspaceDir,
    })
    await recursive(allProjects, [], {
      ...opts,
      ...OVERWRITE_UPDATE_OPTIONS,
      include,
      selectedProjectsGraph,
      workspaceDir: opts.workspaceDir, // Otherwise TypeScript doesn't understant that is is not undefined
    }, 'install')

    if (opts.ignoreScripts) return

    await rebuild(
      [
        {
          buildIndex: 0,
          manifest: await readProjectManifestOnly(opts.dir, opts),
          rootDir: opts.dir,
        },
      ], {
        ...opts,
        pending: true,
        storeController: store.ctrl,
        storeDir: store.dir,
      },
    )
  }
}
