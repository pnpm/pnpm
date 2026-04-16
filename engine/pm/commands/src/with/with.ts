import fs from 'node:fs'
import path from 'node:path'

import { isExecutedByCorepack, packageManager } from '@pnpm/cli.meta'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { resolvePackageManagerIntegrities } from '@pnpm/installing.env-installer'
import { prependDirsToPath } from '@pnpm/shell.path'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import crossSpawn from 'cross-spawn'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { installPnpmToStore } from '../self-updater/installPnpm.js'

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

  fs.mkdirSync(opts.pnpmHomeDir, { recursive: true })
  const store = await createStoreController(opts)
  let binDir: string
  try {
    // resolvePackageManagerIntegrities resolves ranges/dist-tags via the
    // registry and writes the resolved exact version to the envLockfile.
    const envLockfile = await resolvePackageManagerIntegrities(spec, {
      rootDir: opts.pnpmHomeDir,
      registries: opts.registries,
      storeController: store.ctrl,
      storeDir: store.dir,
    })
    const resolvedVersion = envLockfile.importers['.'].packageManagerDependencies?.['pnpm']?.version
    if (!resolvedVersion) {
      throw new PnpmError('CANNOT_RESOLVE_PNPM', `Cannot resolve pnpm version for "${spec}"`)
    }
    ;({ binDir } = await installPnpmToStore(resolvedVersion, {
      envLockfile,
      storeController: store.ctrl,
      storeDir: store.dir,
      registries: opts.registries,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      packageManager: { name: packageManager.name, version: packageManager.version },
    }))
  } finally {
    await store.ctrl.close()
  }

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
    // Best-effort: try to terminate with the same signal the child received.
    // If the signal is handled or ignored, fall back to a non-zero exit code
    // so the caller doesn't mistake an interrupted run for a successful one.
    process.kill(process.pid, signal)
    return { exitCode: 1 }
  }
  return { exitCode: status ?? 0 }
}
