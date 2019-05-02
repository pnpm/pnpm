import runLifecycleHooks from '@pnpm/lifecycle'
import { readImporterManifestOnly } from '@pnpm/read-importer-manifest'
import { realNodeModulesDir } from '@pnpm/utils'

export default async function (
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
  command: string,
) {
  const manifest = await readImporterManifestOnly(opts.prefix)
  const scriptName = args[0]
  if (!manifest.scripts || !manifest.scripts[scriptName]) {
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
  if (manifest.scripts[`pre${scriptName}`]) {
    await runLifecycleHooks(`pre${scriptName}`, manifest, lifecycleOpts)
  }
  await runLifecycleHooks(scriptName, manifest, lifecycleOpts)
  if (manifest.scripts[`post${scriptName}`]) {
    await runLifecycleHooks(`post${scriptName}`, manifest, lifecycleOpts)
  }
}
