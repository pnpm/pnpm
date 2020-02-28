import { docsUrl, readProjectManifest } from '@pnpm/cli-utils'
import { Config, types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
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
import renderHelp = require('render-help')
import writeJsonFile = require('write-json-file')
import { getCurrentBranch, isGitRepo, isRemoteHistoryClean, isWorkingTreeClean } from './gitChecks'
import recursivePublish, { PublishRecursiveOpts } from './recursivePublish'

export function rcOptionsTypes () {
  return {
    ...cliOptionsTypes(),
    ...R.pick([
      'npm-path',
    ], allTypes),
  }
}

export function cliOptionsTypes () {
  return R.pick([
    'access',
    'git-checks',
    'otp',
    'publish-branch',
    'tag',
    'unsafe-perm',
  ], allTypes)
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
            description: 'Checks if current branch is your publish branch, clean and update to date',
            name: '--git-checks',
          },
          {
            description: 'Sets branch name to publish. Default is master',
            name: '--publish-branch',
          },
        ],
      },
    ],
    url: docsUrl('publish'),
    usages: ['pnpm publish [<tarball>|<dir>] [--tag <tag>] [--access <public|restricted>] [options]'],
  })
}

export async function handler (
  args: string[],
  opts: Omit<PublishRecursiveOpts, 'workspaceDir'> & {
    argv: {
      original: string[],
    },
    engineStrict?: boolean,
    recursive?: boolean,
    workspaceDir?: string,
  } & Pick<Config, 'allProjects' | 'gitChecks' | 'publishBranch' | 'selectedProjectsGraph' >,
) {
  if (opts.gitChecks && await isGitRepo()) {
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
    const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
    await recursivePublish(pkgs, {
      ...opts,
      workspaceDir: opts.workspaceDir ?? process.cwd(),
    })
    return
  }
  if (args.length && args[0].endsWith('.tgz')) {
    await runNpm(opts.npmPath, ['publish', ...args])
    return
  }
  const dir = args.length && args[0] || process.cwd()

  let _status!: number
  await fakeRegularManifest(
    {
      dir,
      engineStrict: opts.engineStrict,
      workspaceDir: opts.workspaceDir || dir,
    },
    async () => {
      const { status } = await runNpm(opts.npmPath, ['publish', ...opts.argv.original.slice(1)])
      _status = status!
    },
  )
  if (_status !== 0) {
    process.exit(_status)
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
  fn: () => Promise<void>,
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
  await fn()
  if (replaceManifest) {
    await rimraf(path.join(opts.dir, 'package.json'))
    await writeProjectManifest(manifest, true)
  }
  await Promise.all(
    copiedLicenses.map((copiedLicense) => fs.unlink(copiedLicense)),
  )
}

async function makePublishManifest (dir: string, originalManifest: ProjectManifest) {
  const publishManifest = {
    ...originalManifest,
    dependencies: await makePublishDependencies(dir, originalManifest.dependencies),
    devDependencies: await makePublishDependencies(dir, originalManifest.devDependencies),
    optionalDependencies: await makePublishDependencies(dir, originalManifest.optionalDependencies),
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
        ]),
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
          `because this dependency is not installed. Try running "pnpm install".`,
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
      }),
  )
  return copiedLicenses
}
