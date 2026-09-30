import path from 'node:path'

import { confirm } from '@inquirer/prompts'
import { FILTERING } from '@pnpm/cli.common-cli-options-help'
import { docsUrl, readProjectManifest } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { isError, PnpmError } from '@pnpm/error'
import { runLifecycleHook, type RunLifecycleHookOptions } from '@pnpm/exec.lifecycle'
import { getCurrentBranch, isGitRepo, isHeadDetached, isRemoteHistoryClean, isWorkingTreeClean } from '@pnpm/network.git-utils'
import type { ExportedManifest } from '@pnpm/releasing.exportable-manifest'
import type { ProjectManifest } from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'
import { pick } from 'ramda'
import { realpathMissing } from 'realpath-missing'
import { renderHelp } from 'render-help'

import { extractPublishManifestFromPacked, isTarballPath, type TarballPath } from './extractManifestFromPacked.js'
import { optionsWithOtpEnv } from './otpEnv.js'
import * as pack from './pack.js'
import { publishPackedPkg, type PublishSummary } from './publishPackedPkg.js'
import { type PublishRecursiveOpts, recursivePublish, type RecursivePublishedPackage } from './recursivePublish.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'access',
    'git-checks',
    'ignore-scripts',
    'skip-manifest-obfuscation',
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
    batch: Boolean,
    'dry-run': Boolean,
    force: Boolean,
    json: Boolean,
    otp: String,
    recursive: Boolean,
    'report-summary': Boolean,
  }
}

export const commandNames = ['publish']

const PUBLISH_OPTIONS_HELP = [
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
    description: 'Skip pnpm\'s manifest obfuscation: keep the original `packageManager` field and publish lifecycle scripts in the published manifest instead of stripping them. The pnpm-specific `pnpm` field is still omitted.',
    name: '--skip-manifest-obfuscation',
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
  {
    description: 'Send all packages to the registry in a single request instead of one request per package. Requires --recursive and a registry that implements the "/-/pnpm/v1/publish" endpoint (for example, pnpr)',
    name: '--batch',
  },
]

export function help (): string {
  return renderHelp({
    description: 'Publishes a package to the npm registry.',
    descriptionLists: [
      {
        title: 'Options',

        list: PUBLISH_OPTIONS_HELP,
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
    json?: boolean
    recursive?: boolean
    workspaceDir?: string
  } & Pick<Config, 'bin' | 'gitChecks' | 'ignoreScripts' | 'pnpmHomeDir' | 'publishBranch' | 'embedReadme' | 'skipManifestObfuscation' | 'versioning'>
  & Pick<ConfigContext, 'allProjects'>,
  params: string[]
): Promise<{ exitCode?: number, output?: string } | undefined> {
  const result = await publish(opts, params)
  // Emit per-package summaries on stdout when --json is set: single object for a single-package
  // publish, array for recursive publish. Mirrors `pnpm pack --json`'s shape choice.
  if (opts.json) {
    if (result?.publishSummary) {
      return { output: JSON.stringify(result.publishSummary, null, 2), exitCode: 0 }
    }
    if (result?.publishedPackages) {
      return { output: JSON.stringify(result.publishedPackages, null, 2), exitCode: result.exitCode ?? 0 }
    }
  }
  if (result?.manifest) return
  return result
}

export interface PublishResult {
  exitCode?: number
  manifest?: ProjectManifest
  publishedManifest?: ExportedManifest
  /** Per-package summary in the npm-CLI `--json` shape; only populated for single-package publish. */
  publishSummary?: PublishSummary
  /** Per-package summaries collected by recursive publish. */
  publishedPackages?: RecursivePublishedPackage[]
}

type PublishCommandOptions = Omit<PublishRecursiveOpts, 'workspaceDir'> & {
  argv: {
    original: string[]
  }
  engineStrict?: boolean
  recursive?: boolean
  workspaceDir?: string
} & Pick<Config, 'bin' | 'gitChecks' | 'ignoreScripts' | 'pnpmHomeDir' | 'publishBranch' | 'embedReadme' | 'packGzipLevel' | 'skipManifestObfuscation' | 'versioning'>
& Pick<ConfigContext, 'allProjects'>

export async function publish (
  opts: PublishCommandOptions,
  params: string[]
): Promise<PublishResult> {
  if (opts.batch && !opts.recursive) {
    throw new PnpmError('BATCH_PUBLISH_REQUIRES_RECURSIVE', '--batch can only be used together with --recursive', {
      hint: 'Run "pnpm publish -r --batch" to publish all workspace packages in a single request.',
    })
  }
  await runGitChecks(opts)
  if (opts.recursive && (opts.selectedProjectsGraph != null)) {
    const { exitCode, publishedPackages } = await recursivePublish({
      ...opts,
      selectedProjectsGraph: opts.selectedProjectsGraph,
      workspaceDir: opts.workspaceDir ?? process.cwd(),
    })
    return { exitCode, publishedPackages }
  }

  opts = optionsWithOtpEnv(opts, process.env)

  const dirInParams = (params.length > 0) ? params[0] : undefined

  if (dirInParams != null && isTarballPath(dirInParams)) {
    return publishTarball(dirInParams, opts)
  }

  return publishDir(dirInParams ?? opts.dir ?? process.cwd(), opts)
}

async function runGitChecks (opts: Pick<PublishCommandOptions, 'ci' | 'gitChecks' | 'publishBranch'>): Promise<void> {
  if (opts.gitChecks === false || !(await isGitRepo())) return
  if (!(await isWorkingTreeClean())) {
    throw new PnpmError('GIT_UNCLEAN', 'Unclean working tree. Commit or stash changes first.', {
      hint: GIT_CHECKS_HINT,
    })
  }
  const currentBranch = await getCurrentBranch()
  await checkPublishBranch(currentBranch, opts)
  if (currentBranch !== null && !(await isRemoteHistoryClean())) {
    throw new PnpmError('GIT_NOT_LATEST', 'Remote history differs. Please pull changes.', {
      hint: GIT_CHECKS_HINT,
    })
  }
}

async function checkPublishBranch (
  currentBranch: string | null,
  opts: Pick<PublishCommandOptions, 'ci' | 'publishBranch'>
): Promise<void> {
  const branches = opts.publishBranch ? [opts.publishBranch] : ['master', 'main']
  if (currentBranch === null && !(opts.ci && await isHeadDetached())) {
    throw new PnpmError(
      'GIT_UNKNOWN_BRANCH',
      `The Git HEAD may not attached to any branch, but your "publish-branch" is set to "${branches.join('|')}".`,
      {
        hint: GIT_CHECKS_HINT,
      }
    )
  }
  if (currentBranch !== null && !branches.includes(currentBranch)) {
    await confirmPublishFromBranch(currentBranch, branches)
  }
}

async function confirmPublishFromBranch (currentBranch: string, branches: string[]): Promise<void> {
  let isConfirmed: boolean
  try {
    isConfirmed = await confirm({
      message: `You're on branch "${currentBranch}" but your "publish-branch" is set to "${branches.join('|')}". Do you want to continue?`,
    })
  } catch (err: unknown) {
    if (isError(err) && err.name === 'ExitPromptError') {
      isConfirmed = false
    } else {
      throw err
    }
  }

  if (!isConfirmed) {
    throw new PnpmError('GIT_NOT_CORRECT_BRANCH', `Branch is not on '${branches.join('|')}'.`, {
      hint: GIT_CHECKS_HINT,
    })
  }
}

async function publishTarball (tarballPath: TarballPath, opts: PublishCommandOptions): Promise<PublishResult> {
  const publishedManifest = await extractPublishManifestFromPacked(tarballPath)
  // Publishing a pre-built tarball bypasses `pack.api()`, so we don't have the file listing
  // or unpacked size — those summary fields are reported as empty/zero.
  const publishSummary = await publishPackedPkg({
    tarballPath,
    publishedManifest,
    contents: [],
    unpackedSize: 0,
  }, opts)
  return { exitCode: 0, publishSummary }
}

async function publishDir (dir: string, opts: PublishCommandOptions): Promise<PublishResult> {
  const _runScriptsIfPresent = await bindRunScriptsIfPresent(dir, opts)
  const { manifest } = await readProjectManifest(dir, opts)
  // Unfortunately, we cannot support postpack at the moment
  if (!opts.ignoreScripts) {
    await _runScriptsIfPresent([
      'prepublishOnly',
      'prepublish',
    ], manifest)
  }

  const { publishedManifest, publishSummary } = await packAndPublishFromTemporaryDirectory(dir, opts)

  if (!opts.ignoreScripts) {
    await _runScriptsIfPresent([
      'publish',
      'postpublish',
    ], manifest)
  }
  return { manifest, publishedManifest, publishSummary }
}

async function packAndPublishFromTemporaryDirectory (
  dir: string,
  opts: PublishCommandOptions
): Promise<Pick<PublishResult, 'publishedManifest' | 'publishSummary'>> {
  // We have to publish the tarball from another location.
  // Otherwise, npm would publish the package with the package.json file
  // from the current working directory, ignoring the package.json file
  // that was generated and packed to the tarball.
  // tempy resolves os.tmpdir() when loaded, which throws if that directory is missing.
  const { temporaryDirectory } = await import('tempy')
  const packDestination = temporaryDirectory()
  try {
    const packResult = await pack.api({
      ...opts,
      dir,
      packDestination,
      dryRun: false,
    })
    const publishSummary = await publishPackedPkg(packResult, opts)
    return { publishedManifest: packResult.publishedManifest, publishSummary }
  } finally {
    await rimraf(packDestination)
  }
}

export type BoundRunScriptsIfPresent = (scriptNames: string[], manifest: ProjectManifest) => Promise<void>

/**
 * Binds {@link runScriptsIfPresent} to the lifecycle options for running a package's own
 * publish or pack scripts from {@link dir}.
 */
export async function bindRunScriptsIfPresent (
  dir: string,
  opts: Pick<RunLifecycleHookOptions, 'extraBinPaths' | 'extraEnv' | 'userAgent'>
): Promise<BoundRunScriptsIfPresent> {
  return runScriptsIfPresent.bind(null, {
    depPath: dir,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: opts.extraEnv,
    pkgRoot: dir,
    rootModulesDir: await realpathMissing(path.join(dir, 'node_modules')),
    stdio: 'inherit',
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
    userAgent: opts.userAgent,
  })
}

export async function runScriptsIfPresent (
  opts: RunLifecycleHookOptions,
  scriptNames: string[],
  manifest: ProjectManifest
): Promise<void> {
  for (const scriptName of scriptNames) {
    if (!manifest.scripts?.[scriptName]) continue
    await runLifecycleHook(scriptName, manifest, opts) // eslint-disable-line no-await-in-loop -- lifecycle scripts run in their defined order
  }
}
