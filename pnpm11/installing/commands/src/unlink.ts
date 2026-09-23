import path from 'node:path'

import { UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl, tryReadProjectManifest } from '@pnpm/cli.utils'
import { writeSettings } from '@pnpm/config.writer'
import { DEPENDENCIES_FIELDS, type ProjectManifest } from '@pnpm/types'
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

  const removedLinks: Record<string, string> = {}
  for (const selector in opts.overrides) {
    const specifier = opts.overrides[selector]
    if (specifier.startsWith('link:') && (!params?.length || params.includes(selector))) {
      removedLinks[selector] = specifier
      delete opts.overrides[selector]
    }
  }
  let rootProjectManifest = opts.rootProjectManifest
  if (!isEmpty(removedLinks)) {
    await writeSettings({
      workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
      rootProjectManifestDir: opts.rootProjectManifestDir,
      updatedSettings: {
        overrides: isEmpty(opts.overrides) ? undefined : opts.overrides,
      },
    })
    rootProjectManifest = await removeLinkedDependencies(opts, removedLinks) ?? rootProjectManifest
  }
  await install.handler({ ...opts, rootProjectManifest })
  return undefined
}

/**
 * Removes the dependencies that `pnpm link` added to the root project manifest
 * for the given `link:` overrides. A dependency is removed only when it is a
 * `link:` to the same directory as the override, so a `link:` dependency the
 * user declared to another directory is kept.
 */
async function removeLinkedDependencies (
  opts: install.InstallCommandOptions,
  removedLinks: Record<string, string>
): Promise<ProjectManifest | undefined> {
  const { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.rootProjectManifestDir, opts)
  if (manifest == null) return undefined
  const resolveLinkTarget = (specifier: string) => path.resolve(opts.rootProjectManifestDir, specifier.slice('link:'.length))
  let changed = false
  for (const depField of DEPENDENCIES_FIELDS) {
    const deps = manifest[depField]
    if (deps == null) continue
    for (const [name, overrideSpecifier] of Object.entries(removedLinks)) {
      const specifier = deps[name]
      if (specifier?.startsWith('link:') && resolveLinkTarget(specifier) === resolveLinkTarget(overrideSpecifier)) {
        delete deps[name]
        changed = true
        if (isEmpty(deps)) {
          delete manifest[depField]
        }
      }
    }
  }
  if (changed) {
    await writeProjectManifest(manifest)
  }
  return manifest
}
