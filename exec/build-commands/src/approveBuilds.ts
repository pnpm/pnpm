import { type Config } from '@pnpm/config'
import { globalInfo } from '@pnpm/logger'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { lexCompare } from '@pnpm/util.lex-comparator'
import renderHelp from 'render-help'
import { prompt } from 'enquirer'
import chalk from 'chalk'
import { rebuild, type RebuildCommandOpts } from '@pnpm/plugin-commands-rebuild'
import { getAutomaticallyIgnoredBuilds } from './getAutomaticallyIgnoredBuilds'

export type ApproveBuildsCommandOpts = Pick<Config, 'modulesDir' | 'dir' | 'rootProjectManifest' | 'rootProjectManifestDir'>

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
            description: 'Approve dependencies of global packages',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
  })
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    global: Boolean,
  }
}

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export async function handler (opts: ApproveBuildsCommandOpts & RebuildCommandOpts): Promise<void> {
  if (opts.rootProjectManifest == null) return
  const automaticallyIgnoredBuilds = await getAutomaticallyIgnoredBuilds(opts)
  if (!automaticallyIgnoredBuilds?.length) {
    globalInfo('There are no packages awaiting approval')
    return
  }
  const { result } = await prompt({
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
  const buildPackages = result.map(({ value }: { value: string }) => value)
  const ignoredPackages = automaticallyIgnoredBuilds.filter((automaticallyIgnoredBuild) => !buildPackages.includes(automaticallyIgnoredBuild))
  if (ignoredPackages.length) {
    if (opts.rootProjectManifest.pnpm?.ignoredBuiltDependencies == null) {
      opts.rootProjectManifest.pnpm = {
        ...opts.rootProjectManifest.pnpm,
        ignoredBuiltDependencies: sortUniqueStrings(ignoredPackages),
      }
    } else {
      opts.rootProjectManifest.pnpm.ignoredBuiltDependencies = sortUniqueStrings([
        ...opts.rootProjectManifest.pnpm.ignoredBuiltDependencies,
        ...ignoredPackages,
      ])
    }
  }
  if (buildPackages.length) {
    if (opts.rootProjectManifest.pnpm?.onlyBuiltDependencies == null) {
      opts.rootProjectManifest.pnpm = {
        ...opts.rootProjectManifest.pnpm,
        onlyBuiltDependencies: sortUniqueStrings(buildPackages),
      }
    } else {
      opts.rootProjectManifest.pnpm.onlyBuiltDependencies = sortUniqueStrings([
        ...opts.rootProjectManifest.pnpm.onlyBuiltDependencies,
        ...buildPackages,
      ])
    }
  }
  if (buildPackages.length) {
    const confirmed = await prompt<{ build: boolean }>({
      type: 'confirm',
      name: 'build',
      message: `The next packages will now be built: ${buildPackages.join(', ')}.
Do you approve?`,
      initial: false,
    })
    if (!confirmed.build) {
      return
    }
  }
  const { writeProjectManifest } = await readProjectManifest(opts.rootProjectManifestDir)
  await writeProjectManifest(opts.rootProjectManifest)
  if (buildPackages.length) {
    return rebuild.handler(opts, buildPackages)
  }
}

function sortUniqueStrings (array: string[]): string[] {
  return Array.from(new Set(array)).sort(lexCompare)
}
