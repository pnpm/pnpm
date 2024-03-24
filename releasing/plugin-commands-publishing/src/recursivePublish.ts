import path from 'node:path'

import pFilter from 'p-filter'
import pick from 'ramda/src/pick'
import writeJsonFile from 'write-json-file'

import type {
  Config,
  Project,
  Registries,
  ResolveFunction,
  PublishRecursiveOpts,
} from '@pnpm/types'
import { logger } from '@pnpm/logger'
import { createResolver } from '@pnpm/client'
import { sortPackages } from '@pnpm/sort-packages'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'

import { publish } from './publish.js'

export async function recursivePublish(
  opts: PublishRecursiveOpts & Required<Pick<Config, 'selectedProjectsGraph'>>
): Promise<{ exitCode: number }> {
  const pkgs = Object.values(opts.selectedProjectsGraph ?? {}).map(
    (wsPkg: {
      dependencies: string[];
      package: Project;
    }): Project => {
      return wsPkg.package;
    }
  )

  const resolve = createResolver({
    ...opts,
    authConfig: opts.rawConfig,
    userConfig: opts.userConfig,
    retry: {
      factor: opts.fetchRetryFactor ?? 0,
      maxTimeout: opts.fetchRetryMaxtimeout ?? 0,
      minTimeout: opts.fetchRetryMintimeout ?? 0,
      retries: opts.fetchRetries ?? 0,
      randomize: false,
    },
    timeout: opts.fetchTimeout ?? 0,
  })

  const pkgsToPublish = await pFilter(pkgs, async (pkg) => {
    if (!pkg.manifest?.name || !pkg.manifest.version || pkg.manifest.private) {
      return false
    }

    if (opts.force) {
      return true
    }

    return !(await isAlreadyPublished(
      {
        dir: pkg.dir,
        lockfileDir: opts.lockfileDir ?? pkg.dir,
        registries: opts.registries,
        resolve,
      },
      pkg.manifest.name,
      pkg.manifest.version
    ))
  })

  const publishedPkgDirs = new Set(pkgsToPublish.map(({ dir }) => dir))

  const publishedPackages: Array<{ name?: string; version?: string }> = []

  if (publishedPkgDirs.size === 0) {
    logger.info({
      message: 'There are no new packages that should be published',
      prefix: opts.dir,
    })
  } else {
    const appendedArgs: string[] = []

    if (opts.cliOptions.access) {
      appendedArgs.push(`--access=${opts.cliOptions.access as string}`)
    }

    if (opts.dryRun) {
      appendedArgs.push('--dry-run')
    }

    if (opts.cliOptions.otp) {
      appendedArgs.push(`--otp=${opts.cliOptions.otp as string}`)
    }

    const chunks = sortPackages(opts.selectedProjectsGraph)

    const tag = opts.tag ?? 'latest'

    for (const chunk of chunks) {
      // NOTE: It should be possible to publish these packages concurrently.
      // However, looks like that requires too much resources for some CI envs.
      // See related issue: https://github.com/pnpm/pnpm/issues/6968
      for (const pkgDir of chunk) {
        if (!publishedPkgDirs.has(pkgDir)) {
          continue
        }

        const pkg = opts.selectedProjectsGraph?.[pkgDir]?.package

        const registry =
          pkg?.manifest?.publishConfig?.registry ??
          pickRegistryForPackage(opts.registries, pkg?.manifest?.name ?? '')

        // eslint-disable-next-line no-await-in-loop
        const publishResult = await publish(
          {
            ...opts,
            dir: pkg?.dir ?? '',
            argv: {
              original: [
                'publish',
                '--tag',
                tag,
                '--registry',
                registry,
                ...appendedArgs,
              ],
            },
            gitChecks: false,
            recursive: false,
          },
          [pkg?.dir ?? '']
        )
        if (publishResult?.manifest != null) {
          publishedPackages.push(
            pick(['name', 'version'], publishResult.manifest)
          )
        } else if (publishResult?.exitCode) {
          return { exitCode: publishResult.exitCode }
        }
      }
    }
  }
  if (opts.reportSummary) {
    await writeJsonFile(
      path.join(opts.lockfileDir ?? opts.dir, 'pnpm-publish-summary.json'),
      { publishedPackages }
    )
  }
  return { exitCode: 0 }
}

async function isAlreadyPublished(
  opts: {
    dir: string
    lockfileDir: string
    registries: Registries
    resolve: ResolveFunction
  },
  pkgName: string,
  pkgVersion: string
) {
  try {
    await opts.resolve(
      { alias: pkgName, pref: pkgVersion },
      {
        lockfileDir: opts.lockfileDir,
        preferredVersions: {},
        projectDir: opts.dir,
        registry: pickRegistryForPackage(opts.registries, pkgName, pkgVersion),
      }
    )
    return true
  } catch (err: any) { // eslint-disable-line
    return false
  }
}
