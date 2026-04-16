import fs from 'node:fs'
import path from 'node:path'

import { isExecutedByCorepack, packageManager } from '@pnpm/cli.meta'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { createResolver } from '@pnpm/installing.client'
import { prependDirsToPath } from '@pnpm/shell.path'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import crossSpawn from 'cross-spawn'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { resolveAndInstallPnpmVersion } from '../self-updater/installPnpm.js'

export const commandNames = ['with']

export const skipPackageManagerCheck = true

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return pick([], allTypes)
}

export function help (): string {
  return renderHelp({
    description: 'Run pnpm with a specific version (or the currently running one), ignoring the "packageManager" and "devEngines.packageManager" fields of the project manifest.',
    descriptionLists: [],
    url: docsUrl('with'),
    usages: [
      'pnpm with current <pnpm args>',
      'pnpm with <version> <pnpm args>',
      'pnpm with next install',
      'pnpm with 10 install',
    ],
  })
}

export type WithCommandOptions = CreateStoreControllerOptions & Pick<Config,
| 'dir'
| 'lockfileDir'
| 'pnpmHomeDir'
| 'virtualStoreDirMaxLength'
> & Pick<ConfigContext,
| 'rootProjectManifestDir'
>

export async function handler (
  opts: WithCommandOptions,
  params: string[]
): Promise<{ exitCode: number }> {
  if (params.length === 0) {
    throw new PnpmError('MISSING_WITH_SPEC', 'Missing version argument. Usage: pnpm with <version|current> <args...>')
  }
  if (isExecutedByCorepack()) {
    throw new PnpmError('CANT_USE_WITH_IN_COREPACK', 'The "pnpm with" command does not work under corepack')
  }
  // `with current` is handled earlier in parseCliArgs.ts, which re-parses it
  // for in-process execution, so this handler only ever sees version/dist-tag specs.
  const [spec, ...args] = params

  const { resolve } = createResolver({ ...opts, configByUri: opts.configByUri })
  const resolution = await resolve({ alias: 'pnpm', bareSpecifier: spec }, {
    lockfileDir: opts.lockfileDir ?? opts.dir,
    preferredVersions: {},
    projectDir: opts.dir,
  })
  if (!resolution?.manifest?.version) {
    throw new PnpmError('CANNOT_RESOLVE_PNPM', `Cannot resolve pnpm version for "${spec}"`)
  }
  const version = resolution.manifest.version

  fs.mkdirSync(opts.pnpmHomeDir, { recursive: true })
  const store = await createStoreController(opts)
  let result!: Awaited<ReturnType<typeof resolveAndInstallPnpmVersion>>
  try {
    result = await resolveAndInstallPnpmVersion(version, {
      rootDir: opts.pnpmHomeDir,
      registries: opts.registries,
      storeController: store.ctrl,
      storeDir: store.dir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      packageManager: { name: packageManager.name, version: packageManager.version },
    })
  } finally {
    await store.ctrl.close()
  }
  if (!result.resolvedVersion) {
    throw new PnpmError('CANNOT_RESOLVE_PNPM', `Cannot resolve pnpm version for "${spec}"`)
  }
  const { binDir } = result

  // The child pnpm must skip the packageManager/devEngines check so the requested
  // version stays active. Two keys are set for backward compatibility:
  //   - `COREPACK_ROOT` is honored by every pnpm release that supports corepack
  //     (older versions skip the pm check whenever this is set).
  //   - `pnpm_config_pm_on_fail=ignore` is the principled override recognized
  //     by pnpm releases that ship the `pmOnFail` setting.
  const pnpmEnv = prependDirsToPath([binDir])
  const spawnEnv: NodeJS.ProcessEnv = {
    ...process.env,
    [pnpmEnv.name]: pnpmEnv.value,
    COREPACK_ROOT: process.env.COREPACK_ROOT ?? 'pnpm-with',
    pnpm_config_pm_on_fail: 'ignore',
  }

  const pnpmBinPath = path.join(binDir, 'pnpm')
  const { status, signal, error } = crossSpawn.sync(pnpmBinPath, args, {
    stdio: 'inherit',
    env: spawnEnv,
  })
  if (error) throw error
  if (signal) {
    process.kill(process.pid, signal)
  }
  return { exitCode: status ?? 0 }
}
