import path from 'node:path'

import { UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl, tryReadProjectManifest } from '@pnpm/cli.utils'
import { writeSettings } from '@pnpm/config.writer'
import type { ProjectManifest } from '@pnpm/types'
import { isEmpty } from 'ramda'
import { renderHelp } from 'render-help'

import * as install from './install.js'

export const cliOptionsTypes = install.cliOptionsTypes

export const rcOptionsTypes = install.rcOptionsTypes

export const commandNames = ['unlink', 'dislink']

export function help (): string {
  return renderHelp({
    aliases: ['dislink'],
    description: 'Removes the link created by `pnpm link` and reinstalls package if it is saved in `package.json`',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Unlink in every package found in subdirectories \
or in every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('unlink'),
    usages: [
      'pnpm unlink (in package dir)',
      'pnpm unlink <pkg>...',
    ],
  })
}

export async function handler (
  opts: install.InstallCommandOptions,
  params: string[]
): Promise<undefined | string> {
  if (!opts.overrides) return 'Nothing to unlink'

  const removedLinks = removeLinkOverrides(opts.overrides, params)
  let rootProjectManifest = opts.rootProjectManifest
  if (!isEmpty(removedLinks)) {
    const selected = new Map(Object.values(opts.selectedProjectsGraph ?? {}).map(({ package: project }) => [path.resolve(project.rootDir), project.manifest]))
    const loaded = new Map(opts.allProjects?.map(project => [path.resolve(project.rootDir), project.manifest]))
    const selectedDirs = opts.selectedProjectsGraph == null ? [opts.dir] : [...selected.keys()]
    const manifests = await Promise.all(selectedDirs.map(async dir =>
      unlinkProject(opts, { dir, manifests: [selected.get(dir), loaded.get(dir)] }, removedLinks)
    ))
    rootProjectManifest = manifests.find(project => project?.dir === opts.rootProjectManifestDir)?.manifest ?? rootProjectManifest
    if (!opts.dryRun) {
      await Promise.all(manifests.map(async project => project?.writeProjectManifest(project.manifest)))
      await writeSettings({
        workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
        rootProjectManifestDir: opts.rootProjectManifestDir,
        updatedSettings: { overrides: isEmpty(opts.overrides) ? undefined : opts.overrides },
      })
    }
  }
  await install.handler({ ...opts, rootProjectManifest, frozenLockfileIfExists: isEmpty(removedLinks) ? opts.frozenLockfileIfExists : false })
  return undefined
}

/**
 * Deletes from `overrides` the `link:` overrides selected by `params` (all of
 * them when `params` is empty) and returns the deleted entries.
 */
function removeLinkOverrides (overrides: Record<string, string>, params: string[]): Record<string, string> {
  const removedLinks: Record<string, string> = {}
  for (const selector in overrides) {
    const specifier = overrides[selector]
    if (specifier.startsWith('link:') && (!params?.length || params.includes(selector))) {
      removedLinks[selector] = specifier
      delete overrides[selector]
    }
  }
  return removedLinks
}

async function unlinkProject (
  opts: install.InstallCommandOptions,
  { dir, manifests }: { dir: string, manifests: Array<ProjectManifest | undefined> },
  removedLinks: Record<string, string>
) {
  const { manifest, writeProjectManifest } = await tryReadProjectManifest(dir, opts)
  const overrideDir = opts.workspaceDir ?? opts.rootProjectManifestDir
  if (manifest == null || !dropLinkedDependencies(manifest, removedLinks, dir, overrideDir)) return undefined
  for (const loaded of manifests) {
    if (loaded) dropLinkedDependencies(loaded, removedLinks, dir, overrideDir)
  }
  return { dir, manifest, writeProjectManifest }
}

function dropLinkedDependencies (manifest: ProjectManifest, removedLinks: Record<string, string>, manifestDir: string, overrideDir: string): boolean {
  const deps = manifest.dependencies
  if (deps == null) return false
  const resolveLinkTarget = (dir: string, specifier: string) => path.resolve(dir, specifier.slice('link:'.length))
  let changed = false
  for (const [name, overrideSpecifier] of Object.entries(removedLinks)) {
    const specifier = deps[name]
    if (specifier?.startsWith('link:') && resolveLinkTarget(manifestDir, specifier) === resolveLinkTarget(overrideDir, overrideSpecifier)) {
      delete deps[name]
      changed = true
    }
  }
  if (changed && isEmpty(deps)) {
    delete manifest.dependencies
  }
  return changed
}
