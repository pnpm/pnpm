import { docsUrl, readProjectManifestOnly } from '@pnpm/cli-utils'
import { FILTERING, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, types as allTypes } from '@pnpm/config'
import { type LogBase } from '@pnpm/logger'
import {
  createStoreController,
  type CreateStoreControllerOptions,
} from '@pnpm/store-connection-manager'
import { type ProjectRootDir } from '@pnpm/types'
import { pick } from 'ramda'
import renderHelp from 'render-help'
import {
  rebuildProjects,
  rebuildSelectedPkgs,
} from './implementation/index.js'
import { recursiveRebuild } from './recursive.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'npm-path',
      'reporter',
      'scripts-prepend-node-path',
      'unsafe-perm',
      'store-dir',
    ], allTypes),
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    pending: Boolean,
    recursive: Boolean,
  }
}

export const commandNames = ['rebuild', 'rb']

export function help (): string {
  return renderHelp({
    aliases: ['rb'],
    description: 'Rebuild a package.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Rebuild every package found in subdirectories \
or every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Rebuild packages that were not build during installation. Packages are not build when installing with the --ignore-scripts flag',
            name: '--pending',
          },
          {
            description: 'The directory in which all the packages are saved on the disk',
            name: '--store-dir <dir>',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('rebuild'),
    usages: ['pnpm rebuild [<pkg> ...]'],
  })
}

export type RebuildCommandOpts = Pick<Config,
  | 'allProjects'
  | 'dir'
  | 'engineStrict'
  | 'hooks'
  | 'lockfileDir'
  | 'nodeLinker'
  | 'rawLocalConfig'
  | 'rootProjectManifest'
  | 'rootProjectManifestDir'
  | 'registries'
  | 'scriptShell'
  | 'selectedProjectsGraph'
  | 'sideEffectsCache'
  | 'sideEffectsCacheReadonly'
  | 'scriptsPrependNodePath'
  | 'shellEmulator'
  | 'workspaceDir'
> &
  CreateStoreControllerOptions &
{
  recursive?: boolean
  reporter?: (logObj: LogBase) => void
  pending: boolean
  skipIfHasSideEffectsCache?: boolean
  neverBuiltDependencies?: string[]
  allowBuilds?: Record<string, boolean | string>
  production?: boolean
  development?: boolean
  optional?: boolean
}

export async function handler (
  opts: RebuildCommandOpts,
  params: string[]
): Promise<void> {
  // We want to ignore the NODE_ENV environment variable and
  // rebuild all packages that are present in node_modules.
  opts.production = true
  opts.development = true
  opts.optional = true
  if (opts.recursive && (opts.allProjects != null) && (opts.selectedProjectsGraph != null) && opts.workspaceDir) {
    await recursiveRebuild(opts.allProjects, params, { ...opts, selectedProjectsGraph: opts.selectedProjectsGraph, workspaceDir: opts.workspaceDir })
    return
  }
  const store = await createStoreController(opts)
  const rebuildOpts = Object.assign(opts, {
    sideEffectsCacheRead: opts.sideEffectsCache ?? opts.sideEffectsCacheReadonly,
    sideEffectsCacheWrite: opts.sideEffectsCache,
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  if (params.length === 0) {
    await rebuildProjects(
      [
        {
          buildIndex: 0,
          manifest: await readProjectManifestOnly(rebuildOpts.dir, opts),
          rootDir: rebuildOpts.dir as ProjectRootDir,
        },
      ],
      rebuildOpts
    )
    return
  }
  await rebuildSelectedPkgs(
    [
      {
        buildIndex: 0,
        manifest: await readProjectManifestOnly(rebuildOpts.dir, opts),
        rootDir: rebuildOpts.dir as ProjectRootDir,
      },
    ],
    params,
    rebuildOpts
  )
}
