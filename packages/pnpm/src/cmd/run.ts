import runLifecycleHooks from '@pnpm/lifecycle'
import { readImporterManifestOnly } from '@pnpm/read-importer-manifest'
import { realNodeModulesDir } from '@pnpm/utils'

export default async function run (
  args: string[],
  opts: {
    prefix: string,
    rawNpmConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
) {
  const manifest = await readImporterManifestOnly(opts.prefix)
  const scriptName = args[0]
  if (scriptName !== 'start' && (!manifest.scripts || !manifest.scripts[scriptName])) {
    const err = new Error(`Missing script: ${scriptName}`)
    err['code'] = 'ERR_PNPM_NO_SCRIPT'
    throw err
  }
  const dashDashIndex = opts.argv.cooked.indexOf('--')
  const lifecycleOpts = {
    args: dashDashIndex === -1 ? [] : opts.argv.cooked.slice(dashDashIndex + 1),
    depPath: opts.prefix,
    pkgRoot: opts.prefix,
    rawNpmConfig: opts.rawNpmConfig,
    rootNodeModulesDir: await realNodeModulesDir(opts.prefix),
    stdio: 'inherit',
    unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
  }
  if (manifest.scripts && manifest.scripts[`pre${scriptName}`]) {
    await runLifecycleHooks(`pre${scriptName}`, manifest, lifecycleOpts)
  }
  await runLifecycleHooks(scriptName, manifest, lifecycleOpts)
  if (manifest.scripts && manifest.scripts[`post${scriptName}`]) {
    await runLifecycleHooks(`post${scriptName}`, manifest, lifecycleOpts)
  }
}

export async function start (
  args: string[],
  opts: {
    prefix: string,
    rawNpmConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
) {
  return run(['start', ...args], opts)
}

export async function stop (
  args: string[],
  opts: {
    prefix: string,
    rawNpmConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
) {
  return run(['stop', ...args], opts)
}

export async function test (
  args: string[],
  opts: {
    prefix: string,
    rawNpmConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
) {
  return run(['test', ...args], opts)
}

export async function restart (
  args: string[],
  opts: {
    prefix: string,
    rawNpmConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
) {
  await stop(args, opts)
  await run(['restart', ...args], opts)
  await start(args, opts)
}
