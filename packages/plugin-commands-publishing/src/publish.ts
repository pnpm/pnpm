import { promises as fs, existsSync } from 'fs'
import path from 'path'
import { docsUrl, readProjectManifest } from '@pnpm/cli-utils'
import { Config, types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import runLifecycleHooks, { RunLifecycleHookOptions } from '@pnpm/lifecycle'
import runNpm from '@pnpm/run-npm'
import { ProjectManifest } from '@pnpm/types'
import { prompt } from 'enquirer'
import rimraf from '@zkochan/rimraf'
import pick from 'ramda/src/pick'
import realpathMissing from 'realpath-missing'
import renderHelp from 'render-help'
import * as pack from './pack'
import recursivePublish, { PublishRecursiveOpts } from './recursivePublish'
import { getCurrentBranch, isGitRepo, isRemoteHistoryClean, isWorkingTreeClean } from './gitChecks'

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
            description: "Don't check if current branch is your publish branch, clean, and up-to-date",
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
        ],
      },
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
  } & Pick<Config, 'allProjects' | 'gitChecks' | 'ignoreScripts' | 'publishBranch'>,
  params: string[]
) {
  if (opts.gitChecks !== false && await isGitRepo()) {
    if (!(await isWorkingTreeClean())) {
      throw new PnpmError('GIT_NOT_UNCLEAN', 'Unclean working tree. Commit or stash changes first.', {
        hint: GIT_CHECKS_HINT,
      })
    }
    const branches = opts.publishBranch ? [opts.publishBranch] : ['master', 'main']
    const currentBranch = await getCurrentBranch()
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
    runNpm(opts.npmPath, ['publish', ...params])
    return
  }
  const dirInParams = (params.length > 0) && params[0]
  const dir = dirInParams || opts.dir || process.cwd()

  const _runScriptsIfPresent = runScriptsIfPresent.bind(null, {
    depPath: dir,
    extraBinPaths: opts.extraBinPaths,
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

  const tarballName = await pack.handler({
    ...opts,
    dir,
  })
  const tarballDir = path.dirname(path.join(dir, tarballName))
  const localNpmrc = path.join(tarballDir, '.npmrc')
  const copyNpmrc = !existsSync(localNpmrc) && opts.workspaceDir && existsSync(path.join(opts.workspaceDir, '.npmrc'))
  if (copyNpmrc && opts.workspaceDir) {
    await fs.copyFile(path.join(opts.workspaceDir, '.npmrc'), localNpmrc)
  }
  const { status } = runNpm(opts.npmPath, ['publish', '--ignore-scripts', path.basename(tarballName), ...args], {
    cwd: tarballDir,
  })
  await rimraf(path.join(dir, tarballName))
  if (copyNpmrc) {
    await rimraf(localNpmrc)
  }

  if (status != null && status !== 0) {
    process.exit(status)
  }
  if (!opts.ignoreScripts) {
    await _runScriptsIfPresent([
      'publish',
      'postpublish',
    ], manifest)
  }
  return { manifest }
}

export async function runScriptsIfPresent (
  opts: RunLifecycleHookOptions,
  scriptNames: string[],
  manifest: ProjectManifest
) {
  for (const scriptName of scriptNames) {
    if (!manifest.scripts?.[scriptName]) continue
    await runLifecycleHooks(scriptName, manifest, opts)
  }
}
