import { docsUrl } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { findWorkspaceDir } from '@pnpm/find-workspace-dir'
import { findWorkspacePackages } from '@pnpm/find-workspace-packages'
import * as install from './install'
import renderHelp from 'render-help'

import path from 'node:path'
import pathExists from 'path-exists'
import loadJsonFile from 'load-json-file'

import { hardLinkDir } from '@pnpm/fs.hard-link-dir'
import { PnpmError } from '@pnpm/error'

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

  const localName = (await loadJsonFile(path.join(dir, 'package.json')))?.['name']
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

    if (!files) {
      const syncFrom = pkg.dir
      const resolvedPackagePath = path.dirname(
        resolvePackagePath(pkg.manifest.name, dir)
      )
      const syncTo = path.join(resolvedPackagePath, packageRelativePath)

      if (await pathExists(syncFrom)) {
        await hardLinkDir(syncFrom, [syncTo])
      }


      continue;
    }

    for (const packageRelativePath of files) {
      const syncFrom = path.join(pkg.dir, packageRelativePath)
      const resolvedPackagePath = path.dirname(
        resolvePackagePath(pkg.manifest.name, dir)
      )
      const syncTo = path.join(resolvedPackagePath, packageRelativePath)

      if (await pathExists(syncFrom)) {
        await hardLinkDir(syncFrom, [syncTo])
      }
    }

  }
}

