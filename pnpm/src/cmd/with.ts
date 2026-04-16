import fs from 'node:fs'
import path from 'node:path'

import { detectIfCurrentPkgIsExecutable, isExecutedByCorepack, packageManager } from '@pnpm/cli.meta'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { installPnpmToStore } from '@pnpm/engine.pm.commands'
import { PnpmError } from '@pnpm/error'
import { createResolver } from '@pnpm/installing.client'
import { resolvePackageManagerIntegrities } from '@pnpm/installing.env-installer'
import { prependDirsToPath } from '@pnpm/shell.path'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import crossSpawn from 'cross-spawn'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

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
  const [spec, ...args] = params

  if (spec === 'current') {
    return spawnCurrentPnpm(args)
  }

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

  // If the resolved version matches the running pnpm, skip the store install and re-exec self.
  if (version === packageManager.version) {
    return spawnCurrentPnpm(args)
  }

  fs.mkdirSync(opts.pnpmHomeDir, { recursive: true })
  const store = await createStoreController(opts)
  const envLockfile = await resolvePackageManagerIntegrities(version, {
    registries: opts.registries,
    rootDir: opts.pnpmHomeDir,
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  const { binDir } = await installPnpmToStore(version, {
    envLockfile,
    storeController: store.ctrl,
    storeDir: store.dir,
    registries: opts.registries,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    packageManager: { name: packageManager.name, version: packageManager.version },
  })

  await store.ctrl.close()

  const pnpmEnv = prependDirsToPath([binDir])
  const spawnEnv: NodeJS.ProcessEnv = {
    ...process.env,
    [pnpmEnv.name]: pnpmEnv.value,
    ...childBypassEnv(),
  }

  const pnpmBinPath = path.join(binDir, 'pnpm')
  return runChild(pnpmBinPath, args, spawnEnv)
}

function spawnCurrentPnpm (args: string[]): { exitCode: number } {
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    ...childBypassEnv(),
  }
  if (detectIfCurrentPkgIsExecutable()) {
    return runChild(process.execPath, args, env)
  }
  const entry = process.argv[1]
  if (!entry) {
    throw new PnpmError('CANNOT_LOCATE_PNPM_ENTRY', 'Unable to determine the current pnpm entry script')
  }
  return runChild(process.execPath, [entry, ...args], env)
}

// The child pnpm must skip the packageManager/devEngines check so the requested
// version stays active. Two keys are set for backward compatibility:
//   - `COREPACK_ROOT` is honored by every pnpm release that supports corepack
//     (older versions skip the pm check whenever this is set).
//   - `pnpm_config_package_manager_on_fail=ignore` is the principled override
//     recognized by pnpm releases that ship the `packageManagerOnFail` setting.
function childBypassEnv (): NodeJS.ProcessEnv {
  return {
    COREPACK_ROOT: process.env.COREPACK_ROOT ?? 'pnpm-with',
    pnpm_config_package_manager_on_fail: 'ignore',
  }
}

function runChild (cmd: string, args: string[], env: NodeJS.ProcessEnv): { exitCode: number } {
  const { status, signal, error } = crossSpawn.sync(cmd, args, {
    stdio: 'inherit',
    env,
  })
  if (error) throw error
  if (signal) {
    process.kill(process.pid, signal)
  }
  return { exitCode: status ?? 0 }
}
