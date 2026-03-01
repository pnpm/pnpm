import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
import { type StrictModules, writeModulesManifest } from '@pnpm/modules-yaml'
import { lexCompare } from '@pnpm/util.lex-comparator'
import renderHelp from 'render-help'
import enquirer from 'enquirer'
import chalk from 'chalk'
import { rebuild, type RebuildCommandOpts } from '@pnpm/plugin-commands-rebuild'
import { writeSettings } from '@pnpm/config.config-writer'
import { getAutomaticallyIgnoredBuilds } from './getAutomaticallyIgnoredBuilds.js'

export type ApproveBuildsCommandOpts = Pick<Config, 'modulesDir' | 'dir' | 'rootProjectManifest' | 'rootProjectManifestDir' | 'allowBuilds'> & { all?: boolean, global?: boolean }

export const commandNames = ['approve-builds']

export function help (): string {
  return renderHelp({
    description: 'Approve dependencies for running scripts during installation',
    usages: [],
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

export async function handler (opts: ApproveBuildsCommandOpts & RebuildCommandOpts): Promise<void> {
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
  const {
    automaticallyIgnoredBuilds,
    modulesDir,
    modulesManifest,
  } = await getAutomaticallyIgnoredBuilds(opts)
  if (!automaticallyIgnoredBuilds?.length) {
    globalInfo('There are no packages awaiting approval')
    return
  }
  let buildPackages: string[] = []
  if (opts.all) {
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
  const ignoredPackages = automaticallyIgnoredBuilds.filter((automaticallyIgnoredBuild) => !buildPackages.includes(automaticallyIgnoredBuild))
  const allowBuilds: Record<string, boolean | string> = { ...opts.allowBuilds }
  if (ignoredPackages.length) {
    for (const pkg of ignoredPackages) {
      allowBuilds[pkg] = false
    }
  }
  if (buildPackages.length) {
    for (const pkg of buildPackages) {
      allowBuilds[pkg] = true
    }
  }
  if (!opts.all) {
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
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
    updatedSettings: { allowBuilds },
  })
  if (buildPackages.length) {
    return rebuild.handler({
      ...opts,
      allowBuilds,
    }, buildPackages)
  } else if (modulesManifest) {
    delete modulesManifest.ignoredBuilds
    await writeModulesManifest(modulesDir, modulesManifest as StrictModules)
  }
}

function sortUniqueStrings (array: string[]): string[] {
  return Array.from(new Set(array)).sort(lexCompare)
}
