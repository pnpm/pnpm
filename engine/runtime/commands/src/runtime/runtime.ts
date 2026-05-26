import { docsUrl } from '@pnpm/cli.utils'
import type { Config } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { renderHelp } from 'render-help'

export type RuntimeCommandOptions = Pick<Config,
| 'bin'
| 'dir'
| 'global'
| 'pnpmHomeDir'
> & Partial<Pick<Config,
| 'storeDir'
| 'cacheDir'
| 'saveDev'
| 'saveProd'
>>

export const skipPackageManagerCheck = true

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    global: Boolean,
    'save-dev': Boolean,
    'save-prod': Boolean,
  }
}

export const commandNames = ['runtime', 'rt']

export function help (): string {
  return renderHelp({
    description: 'Manage runtimes.',
    descriptionLists: [
      {
        title: 'Commands',
        list: [
          {
            description: 'Installs the specified version of a runtime (e.g. node, deno, bun).',
            name: 'set',
          },
        ],
      },
      {
        title: 'Options',
        list: [
          {
            description: 'Installs the runtime globally',
            name: '--global',
            shortAlias: '-g',
          },
          {
            description: 'Save the runtime to `devEngines.runtime`. This is the default',
            name: '--save-dev',
            shortAlias: '-D',
          },
          {
            description: 'Save the runtime to `engines.runtime`',
            name: '--save-prod',
            shortAlias: '-P',
          },
        ],
      },
    ],
    url: docsUrl('runtime'),
    usages: [
      'pnpm runtime set node 22',
      'pnpm runtime set node 22 -g',
      'pnpm runtime set node lts -g',
      'pnpm runtime set node rc/22 -g',
      'pnpm runtime set deno 2 -g',
      'pnpm runtime set bun latest -g',
    ],
  })
}

export async function handler (opts: RuntimeCommandOptions, params: string[]): Promise<void> {
  if (params.length === 0) {
    throw new PnpmError('RUNTIME_NO_SUBCOMMAND', 'Please specify the subcommand', {
      hint: help(),
    })
  }
  switch (params[0]) {
    case 'set': {
      runtimeSet(opts, params.slice(1))
      return
    }
    default: {
      throw new PnpmError('RUNTIME_UNKNOWN_SUBCOMMAND', `Unknown subcommand: ${params[0]}`, {
        hint: help(),
      })
    }
  }
}

function runtimeSet (opts: RuntimeCommandOptions, params: string[]): void {
  const runtimeName = params[0]?.trim()
  if (!runtimeName) {
    throw new PnpmError('MISSING_RUNTIME_NAME', '"pnpm runtime set <name> <version>" requires a runtime name (e.g. node, deno, bun)')
  }

  const versionSpec = params[1]?.trim()

  const args = ['add', `${runtimeName}@runtime:${versionSpec ?? ''}`]
  // Default to `devEngines.runtime`; the manifest writer maps a
  // `devDependencies.<runtime>: runtime:<version>` entry to it.
  // `saveDev` wins over `saveProd` to match `getSaveType` precedence.
  if (opts.saveDev || !opts.saveProd) {
    args.push('--save-dev')
  } else {
    args.push('--save-prod')
  }
  if (opts.global) {
    args.push('--global')
    if (opts.bin) args.push('--global-bin-dir', opts.bin)
  } else {
    args.push('--ignore-workspace-root-check')
  }
  if (opts.storeDir) args.push('--store-dir', opts.storeDir)
  if (opts.cacheDir) args.push('--cache-dir', opts.cacheDir)
  runPnpmCli(args, { cwd: opts.global ? opts.pnpmHomeDir : opts.dir })
}
