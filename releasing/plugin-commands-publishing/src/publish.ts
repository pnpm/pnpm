import path from 'path'
import { docsUrl, readProjectManifest } from '@pnpm/cli-utils'
import { FILTERING } from '@pnpm/common-cli-options-help'
import { type Config, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { runLifecycleHook, type RunLifecycleHookOptions } from '@pnpm/lifecycle'
import { type ProjectManifest } from '@pnpm/types'
import { getCurrentBranch, isGitRepo, isRemoteHistoryClean, isWorkingTreeClean } from '@pnpm/git-utils'
import enquirer from 'enquirer'
import rimraf from '@zkochan/rimraf'
import { pick } from 'ramda'
import realpathMissing from 'realpath-missing'
import renderHelp from 'render-help'
import { temporaryDirectory } from 'tempy'
import { extractManifestFromPacked, isTarballPath } from './extractManifestFromPacked.js'
import { optionsWithOtpEnv } from './otpEnv.js'
import * as pack from './pack.js'
import { publishPackedPkg } from './publishPackedPkg.js'
import { recursivePublish, type PublishRecursiveOpts } from './recursivePublish.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'access',
    'git-checks',
    'ignore-scripts',
    'provenance',
    'npm-path',
    'otp',
    'publish-branch',
    'registry',
    'tag',
    'unsafe-perm',
    'embed-readme',
  ], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    'dry-run': Boolean,
    force: Boolean,
    json: Boolean,
    otp: String,
    recursive: Boolean,
    'report-summary': Boolean,
  }
}

export const commandNames = ['publish']

export function help (): string {
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
  } & Pick<Config, 'allProjects' | 'bin' | 'gitChecks' | 'ignoreScripts' | 'pnpmHomeDir' | 'publishBranch' | 'embedReadme'>,
  params: string[]
): Promise<{ exitCode?: number } | undefined> {
  const result = await publish(opts, params)
  if (result?.manifest) return
  return result
}

export interface PublishResult {
  exitCode?: number
  manifest?: ProjectManifest
}

export async function publish (
  opts: Omit<PublishRecursiveOpts, 'workspaceDir'> & {
    argv: {
      original: string[]
    }
    engineStrict?: boolean
    recursive?: boolean
    workspaceDir?: string
  } & Pick<Config, 'allProjects' | 'bin' | 'gitChecks' | 'ignoreScripts' | 'pnpmHomeDir' | 'publishBranch' | 'embedReadme' | 'packGzipLevel'>,
  params: string[]
): Promise<PublishResult> {
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
      const { confirm } = await enquirer.prompt({
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
    const { exitCode } = await recursivePublish({
      ...opts,
      selectedProjectsGraph: opts.selectedProjectsGraph,
      workspaceDir: opts.workspaceDir ?? process.cwd(),
    })
    return { exitCode }
  }

  opts = optionsWithOtpEnv(opts, process.env)

  const dirInParams = (params.length > 0) ? params[0] : undefined

  if (dirInParams != null && isTarballPath(dirInParams)) {
    const tarballPath = dirInParams
    const publishedManifest = await extractManifestFromPacked(tarballPath)
    await publishPackedPkg({
      tarballPath,
      publishedManifest,
    }, opts)
    return { exitCode: 0 }
  }

  const dir = dirInParams ?? opts.dir ?? process.cwd()

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
  const packDestination = temporaryDirectory()
  try {
    const packResult = await pack.api({
      ...opts,
      dir,
      packDestination,
      dryRun: false,
    })
    await publishPackedPkg(packResult, opts)
  } finally {
    await rimraf(packDestination)
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
): Promise<void> {
  for (const scriptName of scriptNames) {
    if (!manifest.scripts?.[scriptName]) continue
    await runLifecycleHook(scriptName, manifest, opts) // eslint-disable-line no-await-in-loop
  }
}
