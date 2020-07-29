import { docsUrl, readProjectManifest } from '@pnpm/cli-utils'
import { Config, types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import runLifecycleHooks, { RunLifecycleHookOptions } from '@pnpm/lifecycle'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import runNpm from '@pnpm/run-npm'
import { Dependencies, ProjectManifest } from '@pnpm/types'
import rimraf = require('@zkochan/rimraf')
import cpFile = require('cp-file')
import { prompt } from 'enquirer'
import fg = require('fast-glob')
import fs = require('mz/fs')
import path = require('path')
import R = require('ramda')
import realpathMissing = require('realpath-missing')
import renderHelp = require('render-help')
import writeJsonFile = require('write-json-file')
import { getCurrentBranch, isGitRepo, isRemoteHistoryClean, isWorkingTreeClean } from './gitChecks'
import recursivePublish, { PublishRecursiveOpts } from './recursivePublish'

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
    'json': Boolean,
    'recursive': Boolean,
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

export async function handler (
  opts: Omit<PublishRecursiveOpts, 'workspaceDir'> & {
    argv: {
      original: string[],
    },
    engineStrict?: boolean,
    recursive?: boolean,
    workspaceDir?: string,
  } & Pick<Config, 'allProjects' | 'gitChecks' | 'ignoreScripts' | 'publishBranch'>,
  params: string[]
) {
  if (opts.gitChecks !== false && await isGitRepo()) {
    const branch = opts.publishBranch ?? 'master'
    if (await getCurrentBranch() !== branch) {
      const { confirm } = await prompt({
        message: `You are not on ${branch} branch, do you want to continue?`,
        name: 'confirm',
        type: 'confirm',
      } as any)// tslint:disable-line:no-any

      if (!confirm) {
        throw new PnpmError('GIT_NOT_CORRECT_BRANCH', `Branch is not on '${branch}'.`)
      }
    }
    if (!(await isWorkingTreeClean())) {
      throw new PnpmError('GIT_NOT_UNCLEAN', 'Unclean working tree. Commit or stash changes first.')
    }
    if (!(await isRemoteHistoryClean())) {
      throw new PnpmError('GIT_NOT_LATEST', 'Remote history differs. Please pull changes.')
    }
  }
  if (opts.recursive && opts.selectedProjectsGraph) {
    await recursivePublish({
      ...opts,
      selectedProjectsGraph: opts.selectedProjectsGraph!,
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
      workspaceDir: opts.workspaceDir || dir,
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

// property keys that are copied from publishConfig into the manifest
const PUBLISH_CONFIG_WHITELIST = new Set([
  // manifest fields that may make sense to overwrite
  'bin',
  // https://github.com/stereobooster/package.json#package-bundlers
  'main',
  'module',
  'typings',
  'types',
  'exports',
  'browser',
  'esnext',
  'es2015',
  'unpkg',
  'umd:main',
])

export async function fakeRegularManifest (
  opts: {
    engineStrict?: boolean,
    dir: string,
    workspaceDir: string,
  },
  fn: (publishManifest: ProjectManifest) => Promise<void>
) {
  // If a workspace package has no License of its own,
  // license files from the root of the workspace are used
  const copiedLicenses: string[] = opts.dir !== opts.workspaceDir && (await findLicenses({ cwd: opts.dir })).length === 0
    ? await copyLicenses(opts.workspaceDir, opts.dir) : []

  const { fileName, manifest, writeProjectManifest } = await readProjectManifest(opts.dir, opts)
  const publishManifest = await makePublishManifest(opts.dir, manifest)
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

async function makePublishManifest (dir: string, originalManifest: ProjectManifest) {
  const publishManifest = {
    ...originalManifest,
    dependencies: await makePublishDependencies(dir, originalManifest.dependencies),
    devDependencies: await makePublishDependencies(dir, originalManifest.devDependencies),
    optionalDependencies: await makePublishDependencies(dir, originalManifest.optionalDependencies),
    peerDependencies: await makePublishDependencies(dir, originalManifest.peerDependencies),
  }

  const { publishConfig } = originalManifest
  if (publishConfig) {
    Object.keys(publishConfig)
      .filter(key => PUBLISH_CONFIG_WHITELIST.has(key))
      .forEach(key => {
        publishManifest[key] = publishConfig[key]
      })
  }

  return publishManifest
}

async function makePublishDependencies (dir: string, dependencies: Dependencies | undefined) {
  if (!dependencies) return dependencies
  const publishDependencies: Dependencies = R.fromPairs(
    await Promise.all(
      R.toPairs(dependencies)
        .map(async ([depName, depSpec]) => [
          depName,
          await makePublishDependency(depName, depSpec, dir),
        ])
    ) as any, // tslint:disable-line
  )
  return publishDependencies
}

async function makePublishDependency (depName: string, depSpec: string, dir: string) {
  if (!depSpec.startsWith('workspace:')) {
    return depSpec
  }
  if (depSpec === 'workspace:*') {
    const { manifest } = await tryReadProjectManifest(path.join(dir, 'node_modules', depName))
    if (!manifest || !manifest.version) {
      throw new PnpmError(
        'CANNOT_RESOLVE_WORKSPACE_PROTOCOL',
        `Cannot resolve workspace protocol of dependency "${depName}" ` +
          `because this dependency is not installed. Try running "pnpm install".`
      )
    }
    return manifest.version
  }
  return depSpec.substr(10)
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
