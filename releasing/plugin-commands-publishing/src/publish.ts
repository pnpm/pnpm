import { promises as fs, existsSync } from 'fs'
import path from 'path'
import { docsUrl, readProjectManifest } from '@pnpm/cli-utils'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { type Config, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { runLifecycleHook, type RunLifecycleHookOptions } from '@pnpm/lifecycle'
import { runNpm } from '@pnpm/run-npm'
import { type ProjectManifest } from '@pnpm/types'
import { getCurrentBranch, isGitRepo, isRemoteHistoryClean, isWorkingTreeClean } from '@pnpm/git-utils'
import { prompt } from 'enquirer'
import rimraf from '@zkochan/rimraf'
import pick from 'ramda/src/pick'
import realpathMissing from 'realpath-missing'
import renderHelp from 'render-help'
import tempy from 'tempy'
import * as pack from './pack'
import { recursivePublish, type PublishRecursiveOpts } from './recursivePublish'

export function rcOptionsTypes () {
  return pick([
    'access',
    'git-checks',
    'ignore-scripts',
    'npm-path',
    'otp',
    'publish-branch',
    'registry',
    'tag',
    'unsafe-perm',
    'embed-readme',
  ], allTypes)
}

export function cliOptionsTypes () {
  return {
    ...rcOptionsTypes(),
    'dry-run': Boolean,
    force: Boolean,
    json: Boolean,
    recursive: Boolean,
    'report-summary': Boolean,
  }
}

export const commandNames = ['publish']

export function help () {
  return renderHelp({
    description: 'Publishes a package to the npm registry.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: "Don't check if current branch is your publish branch, clean, and up to date",
            name: '--no-git-checks',
          },
          {
            description: 'Sets branch name to publish. Default is master',
            name: '--publish-branch',
          },
          {
            description: 'Does everything a publish would do except actually publishing to the registry',
            name: '--dry-run',
          },
          {
            description: 'Show information in JSON format',
            name: '--json',
          },
          {
            description: 'Registers the published package with the given tag. By default, the "latest" tag is used.',
            name: '--tag <tag>',
          },
          {
            description: 'Tells the registry whether this package should be published as public or restricted',
            name: '--access <public|restricted>',
          },
          {
            description: 'Ignores any publish related lifecycle scripts (prepublishOnly, postpublish, and the like)',
            name: '--ignore-scripts',
          },
          {
            description: 'Packages are proceeded to be published even if their current version is already in the registry. This is useful when a "prepublishOnly" script bumps the version of the package before it is published',
            name: '--force',
          },
          {
            description: 'Save the list of the newly published packages to "pnpm-publish-summary.json". Useful when some other tooling is used to report the list of published packages.',
            name: '--report-summary',
          },
          {
            description: 'When publishing packages that require two-factor authentication, this option can specify a one-time password',
            name: '--otp',
          },
          {
            description: 'Publish all packages from the workspace',
            name: '--recursive',
            shortAlias: '-r',
          },
        ],
      },
      FILTERING,
    ],
    url: docsUrl('publish'),
    usages: ['pnpm publish [<tarball>|<dir>] [--tag <tag>] [--access <public|restricted>] [options]'],
  })
}

const GIT_CHECKS_HINT = 'If you want to disable Git checks on publish, set the "git-checks" setting to "false", or run again with "--no-git-checks".'

export async function handler (
  opts: Omit<PublishRecursiveOpts, 'workspaceDir'> & {
    argv: {
      original: string[]
    }
    engineStrict?: boolean
    recursive?: boolean
    workspaceDir?: string
  } & Pick<Config, 'allProjects' | 'gitChecks' | 'ignoreScripts' | 'publishBranch' | 'embedReadme'>,
  params: string[]
) {
  const result = await publish(opts, params)
  if (result?.manifest) return
  return result
}

export async function publish (
  opts: Omit<PublishRecursiveOpts, 'workspaceDir'> & {
    argv: {
      original: string[]
    }
    engineStrict?: boolean
    recursive?: boolean
    workspaceDir?: string
  } & Pick<Config, 'allProjects' | 'gitChecks' | 'ignoreScripts' | 'publishBranch' | 'embedReadme'>,
  params: string[]
) {
  if (opts.gitChecks !== false && await isGitRepo()) {
    if (!(await isWorkingTreeClean())) {
      throw new PnpmError('GIT_UNCLEAN', 'Unclean working tree. Commit or stash changes first.', {
        hint: GIT_CHECKS_HINT,
      })
    }
    const branches = opts.publishBranch ? [opts.publishBranch] : ['master', 'main']
    const currentBranch = await getCurrentBranch()
    if (currentBranch === null) {
      throw new PnpmError(
        'GIT_UNKNOWN_BRANCH',
        `The Git HEAD may not attached to any branch, but your "publish-branch" is set to "${branches.join('|')}".`,
        {
          hint: GIT_CHECKS_HINT,
        }
      )
    }
    if (!branches.includes(currentBranch)) {
      const { confirm } = await prompt({
        message: `You're on branch "${currentBranch}" but your "publish-branch" is set to "${branches.join('|')}". \
Do you want to continue?`,
        name: 'confirm',
        type: 'confirm',
      } as any) as any // eslint-disable-line @typescript-eslint/no-explicit-any

      if (!confirm) {
        throw new PnpmError('GIT_NOT_CORRECT_BRANCH', `Branch is not on '${branches.join('|')}'.`, {
          hint: GIT_CHECKS_HINT,
        })
      }
    }
    if (!(await isRemoteHistoryClean())) {
      throw new PnpmError('GIT_NOT_LATEST', 'Remote history differs. Please pull changes.', {
        hint: GIT_CHECKS_HINT,
      })
    }
  }
  if (opts.recursive && (opts.selectedProjectsGraph != null)) {
    await recursivePublish({
      ...opts,
      selectedProjectsGraph: opts.selectedProjectsGraph,
      workspaceDir: opts.workspaceDir ?? process.cwd(),
    })
    return
  }
  if ((params.length > 0) && params[0].endsWith('.tgz')) {
    const { status } = runNpm(opts.npmPath, ['publish', ...params])
    return { exitCode: status ?? 0 }
  }
  const dirInParams = (params.length > 0) && params[0]
  const dir = dirInParams || opts.dir || process.cwd()

  const _runScriptsIfPresent = runScriptsIfPresent.bind(null, {
    depPath: dir,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: opts.extraEnv,
    pkgRoot: dir,
    rawConfig: opts.rawConfig,
    rootModulesDir: await realpathMissing(path.join(dir, 'node_modules')),
    stdio: 'inherit',
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
  })
  const { manifest } = await readProjectManifest(dir, opts)
  // Unfortunately, we cannot support postpack at the moment
  let args = opts.argv.original.slice(1)
  if (dirInParams) {
    args = args.filter(arg => arg !== params[0])
  }
  const index = args.indexOf('--publish-branch')
  if (index !== -1) {
    // If --publish-branch follows with another cli option, only remove this argument
    // otherwise remove the following argument as well
    if (args[index + 1]?.startsWith('-')) {
      args.splice(index, 1)
    } else {
      args.splice(index, 2)
    }
  }
  if (!opts.ignoreScripts) {
    await _runScriptsIfPresent([
      'prepublishOnly',
      'prepublish',
    ], manifest)
  }

  // We have to publish the tarball from another location.
  // Otherwise, npm would publish the package with the package.json file
  // from the current working directory, ignoring the package.json file
  // that was generated and packed to the tarball.
  const packDestination = tempy.directory()
  const tarballName = await pack.handler({
    ...opts,
    dir,
    packDestination,
  })
  await copyNpmrc({ dir, workspaceDir: opts.workspaceDir, packDestination })
  const { status } = runNpm(opts.npmPath, ['publish', '--ignore-scripts', path.basename(tarballName), ...args], {
    cwd: packDestination,
  })
  await rimraf(packDestination)

  if (status != null && status !== 0) {
    return { exitCode: status }
  }
  if (!opts.ignoreScripts) {
    await _runScriptsIfPresent([
      'publish',
      'postpublish',
    ], manifest)
  }
  return { manifest }
}

async function copyNpmrc (
  { dir, workspaceDir, packDestination }: {
    dir: string
    workspaceDir?: string
    packDestination: string
  }
) {
  const localNpmrc = path.join(dir, '.npmrc')
  if (existsSync(localNpmrc)) {
    await fs.copyFile(localNpmrc, path.join(packDestination, '.npmrc'))
    return
  }
  if (!workspaceDir) return
  const workspaceNpmrc = path.join(workspaceDir, '.npmrc')
  if (existsSync(workspaceNpmrc)) {
    await fs.copyFile(workspaceNpmrc, path.join(packDestination, '.npmrc'))
  }
}

export async function runScriptsIfPresent (
  opts: RunLifecycleHookOptions,
  scriptNames: string[],
  manifest: ProjectManifest
) {
  for (const scriptName of scriptNames) {
    if (!manifest.scripts?.[scriptName]) continue
    await runLifecycleHook(scriptName, manifest, opts)
  }
}
