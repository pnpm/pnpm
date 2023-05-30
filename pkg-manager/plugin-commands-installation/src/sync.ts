import { docsUrl } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { findWorkspaceDir } from '@pnpm/find-workspace-dir'
import { findWorkspacePackages } from '@pnpm/find-workspace-packages'
import type * as install from './install'
import renderHelp from 'render-help'
import resolvePackagePath from 'resolve-package-path'
import Debug from 'debug'
import lockfile from 'proper-lockfile'

import fs from 'node:fs/promises'
import path from 'node:path'
import pathExists from 'path-exists'

import { hardLinkDir } from '@pnpm/fs.hard-link-dir'
import { PnpmError } from '@pnpm/error'
import { readExactProjectManifest } from '@pnpm/read-project-manifest'

const debug = Debug('pnpm:sync')

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {}
}

export const commandNames = ['sync']

export function help () {
  return renderHelp({
    description: 'Re-sync injected dependencies. When ran from a workspace, only that workspaces dependencies will be synchronized. This is useful when using pnpm with external tools such as turborepo, where injected dependencies may need to be synchronized to the .pnpm directory after the external tool restores the build output for packages',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('sync'),
    usages: ['pnpm sync'],
  })
}

export async function handler (opts: install.InstallCommandOptions) {
  const dir = process.cwd()
  const root = await findWorkspaceDir(dir)
  if (!root) {
    throw new PnpmError('NO_ROOT', 'Could not find workspace root')
  }

  const localProjects = await findWorkspacePackages(root)
  const localManifestPath = path.join(dir, 'package.json')
  const localManifest = await readExactProjectManifest(localManifestPath)

  const localName = localManifest['name']
  const ownProject = localProjects.find(project => project.manifest.name === localName)

  if (!ownProject) {
    throw new PnpmError('INVALID_PROJECT', 'Could not find package.json for current directory')
  }

  const ownPackageJson = ownProject.manifest

  const ownDependencies = [
    ...Object.keys(ownPackageJson.dependencies ?? {}),
    ...Object.keys(ownPackageJson.devDependencies ?? {}),
  ]

  const packagesToSync = localProjects.filter((p) => {
    if (!p.manifest.name) return false

    return ownDependencies.includes(p.manifest.name)
  })

  for (const pkg of packagesToSync) {
    const name = pkg.manifest.name
    /**
      * It's not worth trying to sync dpendencies without a name.
      * How would the be depended on, anyway?
      */
    if (!name) continue

    /**
      * We only need to sync files, because
      * these are the only things that would be available in the npm package when published
      *
      * TODO: combine exports and files globs
      *
      * NOTE: some packages may not have either of these, in which case,
      * we'd have to fallback to syncing the whole package
      */
    const { files } = pkg.manifest
    const syncFrom = pkg.dir

    if (!files && !pkg.manifest.exports) {
      // TODO: sync the whole package
      continue
    }

    if (!files) {
      const resolvedPackagePath = resolveDepPath(name, dir)
      const packageRelativePath = path.relative(pkg.dir, resolvedPackagePath)
      const syncTo = path.join(resolvedPackagePath, packageRelativePath)

      if (await pathExists(syncFrom)) {
        await hardLinkDir(syncFrom, [syncTo])
      }

      continue
    }

    await syncFiles(files, name, { from: syncFrom, to: dir })
  }
}

async function syncFiles (files: string[], name: string, { from: fromDir, to: toDir }: { from: string, to: string }) {
  for (const packageRelativePath of files) {
    const syncFrom = path.join(fromDir, packageRelativePath)
    const resolvedPackagePath = resolveDepPath(name, toDir)
    const syncTo = path.join(resolvedPackagePath, packageRelativePath)

    if (await pathExists(syncFrom)) {
      let releaseLock
      try {
        releaseLock = await lockfile.lock(syncTo, { realpath: false })
        debug(`lockfile created for syncing to ${syncTo}`)
      } catch (e) {
        debug(
          `lockfile already exists for syncing to ${syncTo}, some other sync process is already handling this directory, so skipping...`
        )
        continue
      }

      if (await pathExists(syncTo)) {
        await fs.rm(syncTo, { recursive: true })
        debug(`removed ${syncTo} before syncing`)
      }

      debug(`syncing from ${syncFrom} to ${syncTo}`)

      await hardLinkDir(syncFrom, [syncTo])
      releaseLock()
    }
  }
}

function resolveDepPath (name: string, fromDir: string) {
  const resolvedManifestPath = resolvePackagePath(name, fromDir)

  if (!resolvedManifestPath) {
    throw new Error(`Could not resolve package path for ${name}`)
  }

  const resolvedPackagePath = path.dirname(resolvedManifestPath)

  return resolvedPackagePath
}
