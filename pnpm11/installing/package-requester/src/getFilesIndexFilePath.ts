import path from 'node:path'

import { depPathToFilename } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import {
  type AtomicResolution,
  classifyResolution,
  type PlatformAssetResolution,
  resolvePlatformSelector,
  selectPlatformVariant,
  type TarballResolution,
} from '@pnpm/resolving.resolver-base'
import type { FetchPackageToStoreOptions } from '@pnpm/store.controller-types'
import { pickStoreIndexKey } from '@pnpm/store.index'
import type { DepPath, SupportedArchitectures } from '@pnpm/types'
import { familySync } from 'detect-libc'

let currentLibc: 'glibc' | 'musl' | undefined | null
function getLibcFamilySync () {
  if (currentLibc === undefined) {
    currentLibc = familySync() as unknown as typeof currentLibc
  }
  return currentLibc
}

export interface GetFilesIndexFilePathResult {
  target: string
  filesIndexFile: string
  resolution: AtomicResolution
}

export function getFilesIndexFilePath (
  ctx: {
    storeDir: string
    virtualStoreDirMaxLength: number
  },
  opts: Pick<FetchPackageToStoreOptions, 'pkg' | 'ignoreScripts' | 'supportedArchitectures' | 'allowBuild'>
): GetFilesIndexFilePathResult {
  const targetRelative = depPathToFilename(opts.pkg.id, ctx.virtualStoreDirMaxLength)
  const target = path.join(ctx.storeDir, targetRelative)
  let resolution: AtomicResolution
  if (opts.pkg.resolution.type === 'variations') {
    resolution = findResolution(opts.pkg.resolution.variants, opts.supportedArchitectures)
  } else {
    resolution = opts.pkg.resolution
  }
  const denied = opts.pkg.name != null && isGitResolutionKind(classifyResolution(resolution)) &&
    opts.allowBuild?.(`${opts.pkg.name}@${opts.pkg.id}` as DepPath) === false
  const built = !opts.ignoreScripts && !denied
  return {
    target,
    filesIndexFile: pickStoreIndexKey(resolution as TarballResolution, opts.pkg.id, { built }),
    resolution,
  }
}

export function isGitResolutionKind (resolutionKind: ReturnType<typeof classifyResolution>): boolean {
  return resolutionKind === 'git' || resolutionKind === 'gitHostedTarball'
}

function findResolution (resolutionVariants: PlatformAssetResolution[], supportedArchitectures?: SupportedArchitectures): AtomicResolution {
  const selector = resolvePlatformSelector(supportedArchitectures, {
    platform: process.platform,
    arch: process.arch,
    libc: getLibcFamilySync(),
  })
  const variant = selectPlatformVariant(resolutionVariants, selector)
  if (!variant) {
    const resolutionTargets = resolutionVariants.map((variant) => variant.targets)
    throw new PnpmError('NO_RESOLUTION_MATCHED', `Cannot find a resolution variant for the current platform in these resolutions: ${JSON.stringify(resolutionTargets)}`)
  }
  return variant.resolution
}
