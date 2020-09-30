import { docsUrl, readProjectManifest } from '@pnpm/cli-utils'
import { Config, types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import exportableManifest from '@pnpm/exportable-manifest'
import runLifecycleHooks, { RunLifecycleHookOptions } from '@pnpm/lifecycle'
import runNpm from '@pnpm/run-npm'
import { ProjectManifest } from '@pnpm/types'
import { prompt } from 'enquirer'
import { getCurrentBranch, isGitRepo, isRemoteHistoryClean, isWorkingTreeClean } from './gitChecks'
import recursivePublish, { PublishRecursiveOpts } from './recursivePublish'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import cpFile = require('cp-file')
import fg = require('fast-glob')
import fs = require('mz/fs')
import R = require('ramda')
import realpathMissing = require('realpath-missing')
import renderHelp = require('render-help')
import writeJsonFile = require('write-json-file')

export function rcOptionsTypes () {
  return R.pick([
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
    json: Boolean,
    recursive: Boolean,
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
    const branch = opts.publishBranch ?? 'master'
    const currentBranch = await getCurrentBranch()
    if (currentBranch !== branch) {
      const { confirm } = await prompt({
        message: `You're on branch "${currentBranch}" but your "publish-branch" is set to "${branch}". \
Do you want to continue?`,
        name: 'confirm',
        type: 'confirm',
      } as any) as any // eslint-disable-line @typescript-eslint/no-explicit-any

      if (!confirm) {
        throw new PnpmError('GIT_NOT_CORRECT_BRANCH', `Branch is not on '${branch}'.`, {
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
  if (opts.recursive && opts.selectedProjectsGraph) {
    await recursivePublish({
      ...opts,
      selectedProjectsGraph: opts.selectedProjectsGraph,
      workspaceDir: opts.workspaceDir ?? process.cwd(),
    })
    return
  }
  if (params.length && params[0].endsWith('.tgz')) {
    runNpm(opts.npmPath, ['publish', ...params])
    return
  }
  const dir = params.length && params[0] || process.cwd()

  const _runScriptsIfPresent = runScriptsIfPresent.bind(null, {
    depPath: dir,
    extraBinPaths: opts.extraBinPaths,
    pkgRoot: dir,
    rawConfig: opts.rawConfig,
    rootModulesDir: await realpathMissing(path.join(dir, 'node_modules')),
    stdio: 'inherit',
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
  })
  let _status!: number
  await fakeRegularManifest(
    {
      dir,
      engineStrict: opts.engineStrict,
      workspaceDir: opts.workspaceDir ?? dir,
    },
    async (publishManifest) => {
      // Unfortunately, we cannot support postpack at the moment
      if (!opts.ignoreScripts) {
        await _runScriptsIfPresent([
          'prepublish',
          'prepare',
          'prepublishOnly',
          'prepack',
        ], publishManifest)
      }
      const { status } = runNpm(opts.npmPath, ['publish', '--ignore-scripts', ...opts.argv.original.slice(1)])
      if (!opts.ignoreScripts) {
        await _runScriptsIfPresent([
          'publish',
          'postpublish',
        ], publishManifest)
      }
      _status = status!
    }
  )
  if (_status !== 0) {
    process.exit(_status)
  }
}

async function runScriptsIfPresent (
  opts: RunLifecycleHookOptions,
  scriptNames: string[],
  manifest: ProjectManifest
) {
  for (const scriptName of scriptNames) {
    if (!manifest.scripts?.[scriptName]) continue
    await runLifecycleHooks(scriptName, manifest, opts)
  }
}

const LICENSE_GLOB = 'LICEN{S,C}E{,.*}'
const findLicenses = fg.bind(fg, [LICENSE_GLOB]) as (opts: { cwd: string }) => Promise<string[]>

export async function fakeRegularManifest (
  opts: {
    engineStrict?: boolean
    dir: string
    workspaceDir: string
  },
  fn: (publishManifest: ProjectManifest) => Promise<void>
) {
  // If a workspace package has no License of its own,
  // license files from the root of the workspace are used
  const copiedLicenses: string[] = opts.dir !== opts.workspaceDir && (await findLicenses({ cwd: opts.dir })).length === 0
    ? await copyLicenses(opts.workspaceDir, opts.dir) : []

  const { fileName, manifest, writeProjectManifest } = await readProjectManifest(opts.dir, opts)
  const publishManifest = await exportableManifest(opts.dir, manifest)
  const replaceManifest = fileName !== 'package.json' || !R.equals(manifest, publishManifest)
  if (replaceManifest) {
    await rimraf(path.join(opts.dir, fileName))
    await writeJsonFile(path.join(opts.dir, 'package.json'), publishManifest)
  }
  await fn(publishManifest)
  if (replaceManifest) {
    await rimraf(path.join(opts.dir, 'package.json'))
    await writeProjectManifest(manifest, true)
  }
  await Promise.all(
    copiedLicenses.map((copiedLicense) => fs.unlink(copiedLicense))
  )
}

async function copyLicenses (sourceDir: string, destDir: string) {
  const licenses = await findLicenses({ cwd: sourceDir })
  if (licenses.length === 0) return []

  const copiedLicenses: string[] = []
  await Promise.all(
    licenses
      .map((licenseRelPath) => path.join(sourceDir, licenseRelPath))
      .map((licensePath) => {
        const licenseCopyDest = path.join(destDir, path.basename(licensePath))
        copiedLicenses.push(licenseCopyDest)
        return cpFile(licensePath, licenseCopyDest)
      })
  )
  return copiedLicenses
}
