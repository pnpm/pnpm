import type { CommandHandlerMap } from '@pnpm/cli.command'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { writeSettings } from '@pnpm/config.writer'
import { parse } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import { install } from '@pnpm/installing.commands'
import { type StrictModules, writeModulesManifest } from '@pnpm/installing.modules-yaml'
import { globalInfo } from '@pnpm/logger'
import { lexCompare } from '@pnpm/util.lex-comparator'
import chalk from 'chalk'
import enquirer from 'enquirer'
import { renderHelp } from 'render-help'

import { rebuild, type RebuildCommandOpts } from '../build/index.js'
import { getAutomaticallyIgnoredBuilds } from './getAutomaticallyIgnoredBuilds.js'

export type ApproveBuildsCommandOpts = Pick<Config, 'modulesDir' | 'dir' | 'allowBuilds' | 'enableGlobalVirtualStore'> & Pick<ConfigContext, 'rootProjectManifest' | 'rootProjectManifestDir'> & {
  all?: boolean
  global?: boolean
  /**
   * When set, overrides the target directory for writeSettings.
   * Used by the global-install flow to point allowBuilds updates at the
   * global pnpm-workspace.yaml while keeping workspaceDir unset so the
   * install itself targets only the single install directory.
   */
  settingsDir?: string
}

export const commandNames = ['approve-builds']

export const recursiveByDefault = true

export function help (): string {
  return renderHelp({
    description: 'Approve dependencies for running scripts during installation',
    usages: [
      'pnpm approve-builds',
      'pnpm approve-builds [<pkg> ...] [!<pkg> ...]',
    ],
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Approve all pending dependencies without interactive prompts',
            name: '--all',
          },
        ],
      },
    ],
  })
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    all: Boolean,
    global: Boolean,
  }
}

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export async function handler (opts: ApproveBuildsCommandOpts & RebuildCommandOpts, params: string[] = [], commands?: CommandHandlerMap): Promise<void> {
  if (opts.global) {
    throw new PnpmError(
      'APPROVE_BUILDS_NOT_SUPPORTED_WITH_GLOBAL',
      '"approve-builds" is not supported with global packages',
      {
        hint: 'Use --allow-build when installing globally, e.g. "pnpm add -g --allow-build=<pkg> <pkg>". ' +
          'pnpm will also prompt to allow builds interactively during global install.',
      }
    )
  }
  if (opts.all && params.length) {
    throw new PnpmError(
      'APPROVE_BUILDS_ALL_WITH_ARGS',
      'Cannot use --all with positional arguments'
    )
  }
  const {
    automaticallyIgnoredBuilds,
    modulesDir,
    modulesManifest,
  } = await getAutomaticallyIgnoredBuilds(opts)
  if (!automaticallyIgnoredBuilds?.length) {
    globalInfo('There are no packages awaiting approval')
    return
  }
  const denied: string[] = []
  const approved: string[] = []
  const unknown: string[] = []
  for (const p of params) {
    const name = p.startsWith('!') ? p.slice(1) : p
    if (!automaticallyIgnoredBuilds.includes(name)) {
      unknown.push(name)
    } else if (p.startsWith('!')) {
      denied.push(name)
    } else {
      approved.push(name)
    }
  }
  if (unknown.length) {
    throw new PnpmError(
      'APPROVE_BUILDS_UNKNOWN_PACKAGES',
      `The following packages are not awaiting approval: ${unknown.join(', ')}`
    )
  }
  const contradictions = approved.filter((p) => denied.includes(p))
  if (contradictions.length) {
    throw new PnpmError(
      'APPROVE_BUILDS_CONTRADICTING_ARGS',
      `The following packages are both approved and denied: ${contradictions.join(', ')}`
    )
  }
  let buildPackages: string[] = []
  if (params.length) {
    buildPackages = sortUniqueStrings([...approved])
  } else if (opts.all) {
    buildPackages = sortUniqueStrings([...automaticallyIgnoredBuilds])
  } else {
    const { result } = await enquirer.prompt({
      choices: sortUniqueStrings([...automaticallyIgnoredBuilds]),
      indicator (state: any, choice: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
        return ` ${choice.enabled ? '●' : '○'}`
      },
      message: 'Choose which packages to build ' +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)`,
      name: 'result',
      pointer: '❯',
      result () {
        return this.selected
      },
      styles: {
        dark: chalk.reset,
        em: chalk.bgBlack.whiteBright,
        success: chalk.reset,
      },
      type: 'multiselect',

      // For Vim users (related: https://github.com/enquirer/enquirer/pull/163)
      j () {
        return this.down()
      },
      k () {
        return this.up()
      },
      cancel () {
        // By default, canceling the prompt via Ctrl+c throws an empty string.
        // The custom cancel function prevents that behavior.
        // Otherwise, pnpm CLI would print an error and confuse users.
        // See related issue: https://github.com/enquirer/enquirer/issues/225
        process.exit(0)
      },
    } as any) as any // eslint-disable-line @typescript-eslint/no-explicit-any
    buildPackages = result.map(({ value }: { value: string }) => value)
  }
  const allowBuilds: Record<string, boolean | string> = { ...opts.allowBuilds }
  if (params.length) {
    for (const pkg of approved) {
      allowBuilds[pkg] = true
    }
    for (const pkg of denied) {
      allowBuilds[pkg] = false
    }
  } else {
    const ignoredPackages = automaticallyIgnoredBuilds.filter((automaticallyIgnoredBuild) => !buildPackages.includes(automaticallyIgnoredBuild))
    for (const pkg of ignoredPackages) {
      allowBuilds[pkg] = false
    }
    for (const pkg of buildPackages) {
      allowBuilds[pkg] = true
    }
  }
  if (!opts.all && !params.length) {
    if (buildPackages.length) {
      const confirmed = await enquirer.prompt<{ build: boolean }>({
        type: 'confirm',
        name: 'build',
        message: `The next packages will now be built: ${buildPackages.join(', ')}.
Do you approve?`,
        initial: false,
      })
      if (!confirmed.build) {
        return
      }
    } else {
      globalInfo('All packages were added to allowBuilds with value false.')
    }
  }
  await writeSettings({
    ...opts,
    workspaceDir: opts.settingsDir ?? opts.workspaceDir ?? opts.rootProjectManifestDir,
    updatedSettings: { allowBuilds },
  })
  if (modulesManifest?.ignoredBuilds) {
    if (params.length) {
      const decided = new Set([...approved, ...denied])
      for (const depPath of Array.from(modulesManifest.ignoredBuilds)) {
        const name = parse(depPath).name ?? depPath
        if (decided.has(name)) {
          modulesManifest.ignoredBuilds.delete(depPath)
        }
      }
      if (!modulesManifest.ignoredBuilds.size) {
        delete modulesManifest.ignoredBuilds
      }
    } else {
      delete modulesManifest.ignoredBuilds
    }
    await writeModulesManifest(modulesDir, modulesManifest as StrictModules)
  }
  if (buildPackages.length) {
    if (opts.enableGlobalVirtualStore) {
      await install.handler({
        ...opts,
        allowBuilds,
        frozenLockfile: true,
        optimisticRepeatInstall: false,
      } as any, [], commands) // eslint-disable-line @typescript-eslint/no-explicit-any
      return
    }
    return rebuild.handler({
      ...opts,
      allowBuilds,
    }, buildPackages)
  }
}

function sortUniqueStrings (array: string[]): string[] {
  return Array.from(new Set(array)).sort(lexCompare)
}
