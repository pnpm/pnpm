import type { CommandHandlerMap } from '@pnpm/cli.command'
import { UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import { renderHelp } from 'render-help'

import { type InstallCommandOptions, rcOptionsTypes as installCommandRcOptionsTypes } from './install.js'
import { installDeps } from './installDeps.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return installCommandRcOptionsTypes()
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
  }
}

export const shorthands = {}

export const commandNames = ['overrides']

export const recursiveByDefault = true

export function help (): string {
  return renderHelp({
    description: 'Audit `overrides` entries. The default subcommand is `check`.',
    descriptionLists: [
      {
        title: 'Commands',
        list: [
          {
            description: 'Resolve the workspace fully and report every `overrides` entry that matched no dependency. \
Exits with a non-zero exit code when unused overrides are found.',
            name: 'check',
          },
        ],
      },
      {
        title: 'Options',
        list: [
          {
            description: 'Print the result as JSON (`{ "unused": [...] }`).',
            name: '--json',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('overrides'),
    usages: [
      'pnpm overrides',
      'pnpm overrides check',
    ],
  })
}

export type OverridesCommandOptions = InstallCommandOptions & {
  json?: boolean
}

export async function handler (
  opts: OverridesCommandOptions,
  params: string[],
  commands?: CommandHandlerMap
): Promise<string | { output: string, exitCode: number }> {
  switch (params[0]) {
    case 'check':
    case undefined:
      return checkCmd(opts, commands)
    default:
      return { output: help(), exitCode: 1 }
  }
}

/**
 * Resolve fully with the read-package hook and the peer-edge overrider both
 * wired to a Set collector. Because every manifest is read on a forced full
 * resolution, the collector is authoritative — including for transitive
 * version-scoped overrides that the `pnpm install` warning's lockfile scan
 * cannot detect (the resolved lockfile doesn't preserve pre-override declared
 * ranges, so the scan checks name presence only).
 */
async function checkCmd (
  opts: OverridesCommandOptions,
  commands?: CommandHandlerMap
): Promise<{ output: string, exitCode: number }> {
  const appliedSelectors = new Set<string>()
  // `dryRun` returns the wanted lockfile in-memory without writing anything;
  // `forceFullResolution` defeats every short-circuit so the read-package hook
  // fires for every resolved manifest and the peer-edge overrider fires for
  // every resolver-created edge.
  await installDeps({
    ...opts,
    rebuildHandler: commands?.rebuild,
    include: {
      dependencies: opts.production !== false,
      devDependencies: opts.dev !== false,
      optionalDependencies: opts.optional !== false,
    },
    includeDirect: {
      dependencies: opts.production !== false,
      devDependencies: opts.dev !== false,
      optionalDependencies: opts.optional !== false,
    },
    optimisticRepeatInstall: false,
    lockfileOnly: true,
    forceFullResolution: true,
    dryRun: true,
    onAppliedOverride: (selector) => appliedSelectors.add(selector),
  }, [])

  const parsedOverrides = parseOverrides(opts.overrides ?? {}, opts.catalogs ?? {})
  // Convergence overrides (`"pkg@"`) have their own staleness path and never
  // fire the collector — excluding them avoids flagging every convergence
  // override as unused.
  const unused = parsedOverrides
    .filter((override) => override.converge !== true && !appliedSelectors.has(override.selector))
    .map((override) => override.selector)
    .sort()

  if (unused.length === 0) {
    const json = opts.json ?? opts.cliOptions?.['json'] as boolean | undefined
    if (json) {
      return { output: JSON.stringify({ unused: [] }, null, 2), exitCode: 0 }
    }
    return { output: 'No unused overrides', exitCode: 0 }
  }

  const json = opts.json ?? opts.cliOptions?.['json'] as boolean | undefined
  if (json) {
    return {
      output: JSON.stringify({ unused }, null, 2),
      exitCode: 1,
    }
  }
  return {
    output: `${unused.length} unused override${unused.length === 1 ? '' : 's'}:\n${unused.map((s) => `  - ${s}`).join('\n')}`,
    exitCode: 1,
  }
}
