import { PnpmError } from '@pnpm/error'
import { type Finder, type IncludedDependencies } from '@pnpm/types'

export type ReportAs = 'parseable' | 'json' | 'tree'

export function computeInclude (opts: { production?: boolean; dev?: boolean; optional?: boolean }): IncludedDependencies {
  return {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
}

export function resolveFinders (opts: { findBy?: string[]; finders?: Record<string, Finder> }): Finder[] {
  const finders: Finder[] = []
  if (opts.findBy) {
    for (const finderName of opts.findBy) {
      if (opts.finders?.[finderName] == null) {
        throw new PnpmError('FINDER_NOT_FOUND', `No finder with name ${finderName} is found`)
      }
      finders.push(opts.finders[finderName])
    }
  }
  return finders
}

export function determineReportAs (opts: { parseable?: boolean; json?: boolean }): ReportAs {
  return opts.parseable ? 'parseable' : (opts.json ? 'json' : 'tree')
}

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const BASE_RC_OPTION_KEYS = [
  'dev',
  'global-dir',
  'global',
  'json',
  'long',
  'only',
  'optional',
  'parseable',
  'production',
] as const

export const SHARED_CLI_HELP_OPTIONS = [
  {
    description: 'Perform command on every package in subdirectories \
or on every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
    name: '--recursive',
    shortAlias: '-r',
  },
  {
    description: 'Show extended information',
    name: '--long',
  },
  {
    description: 'Show parseable output instead of tree view',
    name: '--parseable',
  },
  {
    description: 'Show information in JSON format',
    name: '--json',
  },
  {
    description: 'List packages in the global install prefix instead of in the current project',
    name: '--global',
    shortAlias: '-g',
  },
  {
    description: 'Display only the dependency graph for packages in `dependencies` and `optionalDependencies`',
    name: '--prod',
    shortAlias: '-P',
  },
  {
    description: 'Display only the dependency graph for packages in `devDependencies`',
    name: '--dev',
    shortAlias: '-D',
  },
  {
    description: "Don't display packages from `optionalDependencies`",
    name: '--no-optional',
  },
]
