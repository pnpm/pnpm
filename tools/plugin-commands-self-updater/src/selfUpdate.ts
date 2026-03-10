import path from 'path'
import { docsUrl } from '@pnpm/cli-utils'
import { packageManager, isExecutedByCorepack } from '@pnpm/cli-meta'
import { createResolver } from '@pnpm/client'
import { type Config, types as allTypes } from '@pnpm/config'
import { resolvePackageManagerIntegrities } from '@pnpm/config.deps-installer'
import { PnpmError } from '@pnpm/error'
import { linkBins } from '@pnpm/link-bins'
import { globalWarn } from '@pnpm/logger'
import { readProjectManifest, tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { pick } from 'ramda'
import renderHelp from 'render-help'
import { installPnpm } from './installPnpm.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
  }
}

export const commandNames = ['self-update']

export function help (): string {
  return renderHelp({
    description: 'Updates pnpm to the latest version (or the one specified)',
    descriptionLists: [],
    url: docsUrl('self-update'),
    usages: [
      'pnpm self-update',
      'pnpm self-update 9',
      'pnpm self-update next-10',
      'pnpm self-update 9.10.0',
    ],
  })
}

export type SelfUpdateCommandOptions = CreateStoreControllerOptions & Pick<Config,
| 'globalPkgDir'
| 'lockfileDir'
| 'managePackageManagerVersions'
| 'modulesDir'
| 'pnpmHomeDir'
| 'rootProjectManifestDir'
| 'wantedPackageManager'
>

export async function handler (
  opts: SelfUpdateCommandOptions,
  params: string[]
): Promise<undefined | string> {
  if (isExecutedByCorepack()) {
    throw new PnpmError('CANT_SELF_UPDATE_IN_COREPACK', 'You should update pnpm with corepack')
  }
  const { resolve } = createResolver({ ...opts, authConfig: opts.rawConfig })
  const pkgName = 'pnpm'
  const bareSpecifier = params[0] ?? 'latest'
  const resolution = await resolve({ alias: pkgName, bareSpecifier }, {
    lockfileDir: opts.lockfileDir ?? opts.dir,
    preferredVersions: {},
    projectDir: opts.dir,
  })
  if (!resolution?.manifest) {
    throw new PnpmError('CANNOT_RESOLVE_PNPM', `Cannot find "${bareSpecifier}" version of pnpm`)
  }

  if (opts.wantedPackageManager?.name === packageManager.name && opts.managePackageManagerVersions) {
    if (opts.wantedPackageManager?.version !== resolution.manifest.version) {
      const { manifest, writeProjectManifest } = await readProjectManifest(opts.rootProjectManifestDir)
      manifest.packageManager = `pnpm@${resolution.manifest.version}`
      await writeProjectManifest(manifest)
      const store = await createStoreController(opts)
      await resolvePackageManagerIntegrities(resolution.manifest.version, {
        registries: opts.registries,
        rootDir: opts.rootProjectManifestDir,
        storeController: store.ctrl,
        storeDir: store.dir,
      })
      return `The current project has been updated to use pnpm v${resolution.manifest.version}`
    } else {
      return `The current project is already set to use pnpm v${resolution.manifest.version}`
    }
  }
  if (resolution.manifest.version === packageManager.version) {
    return `The currently active ${packageManager.name} v${packageManager.version} is already "${bareSpecifier}" and doesn't need an update`
  }

  const store = await createStoreController(opts)

  // Use pnpmHomeDir as fallback for env lockfile when there's no project
  const { manifest: projectManifest, writeProjectManifest } = await tryReadProjectManifest(opts.rootProjectManifestDir)
  const envLockfileDir = projectManifest != null ? opts.rootProjectManifestDir : opts.pnpmHomeDir

  // Resolve integrities and write pnpm-lock.env.yaml
  const envLockfile = await resolvePackageManagerIntegrities(resolution.manifest.version, {
    registries: opts.registries,
    rootDir: envLockfileDir,
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  // Update packageManager field in package.json if it exists
  if (projectManifest != null) {
    projectManifest.packageManager = `pnpm@${resolution.manifest.version}`
    await writeProjectManifest(projectManifest)
  }

  const { baseDir, alreadyExisted } = await installPnpm(resolution.manifest.version, {
    ...opts,
    envLockfile,
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  // Link bins to pnpmHomeDir so the updated pnpm is the active global binary
  await linkBins(path.join(baseDir, 'node_modules'), opts.pnpmHomeDir, { warn: globalWarn })

  if (alreadyExisted) {
    return `The ${bareSpecifier} version, v${resolution.manifest.version}, is already present on the system. It was activated by linking it from ${baseDir}.`
  }
  return undefined
}
