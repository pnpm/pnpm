import { parseIntegrity } from '@pnpm/crypto.integrity'
import { readMsgpackFileSync } from '@pnpm/fs.msgpack-file'
import { type RandomDependency } from '@pnpm/lockfile.fs'
import {
  getFilePathByModeInCafs,
  type PackageFilesIndex,
} from '@pnpm/store.cafs'
import { type PackageManifest } from '@pnpm/types'
import gfs from '@pnpm/graceful-fs'
import path from 'path'

export type FundingType = 'funding' | 'repository' | 'homepage'

export interface FundingInfo {
  packageName: string
  packageDescription?: string
  fundingUrl: string
  fundingType: FundingType
}

function getIndexFilePath (storeDir: string, integrity: string, pkgId: string): string {
  const { hexDigest } = parseIntegrity(integrity)
  const hex = hexDigest.substring(0, 64)
  return path.join(storeDir, `index/${path.join(hex.slice(0, 2), hex.slice(2))}-${pkgId.replace(/[\\/:*?"<>|]/g, '+')}.mpk`)
}

function readManifestFromIndex (storeDir: string, pkgIndex: PackageFilesIndex): PackageManifest | undefined {
  const pkg = pkgIndex.files.get('package.json')
  if (pkg) {
    const fileName = getFilePathByModeInCafs(storeDir, pkg.digest, pkg.mode)
    return JSON.parse(gfs.readFileSync(fileName, 'utf8')) as PackageManifest
  }
  return undefined
}

function isNpmHostedResolution (resolution: unknown): resolution is { integrity: string } {
  // npm-hosted packages have only an integrity field, no 'type' field
  return (
    typeof resolution === 'object' &&
    resolution !== null &&
    'integrity' in resolution &&
    typeof (resolution as { integrity: unknown }).integrity === 'string' &&
    !('type' in resolution)
  )
}

function normalizeRepositoryUrl (repo: string | { url?: string } | undefined): string | undefined {
  if (!repo) return undefined
  const url = typeof repo === 'string' ? repo : repo.url
  if (!url) return undefined
  // Convert git URLs to HTTPS
  return url
    .replace(/^git\+/, '')
    .replace(/^git:\/\//, 'https://')
    .replace(/^git@github\.com:/, 'https://github.com/')
    .replace(/\.git$/, '')
}

function normalizeFundingUrl (funding: string | { url?: string } | undefined): string | undefined {
  if (!funding) return undefined
  return typeof funding === 'string' ? funding : funding.url
}

export function getFundingInfo (
  storeDir: string,
  randomDep: RandomDependency
): FundingInfo | undefined {
  // Only handle npm-hosted packages
  if (!isNpmHostedResolution(randomDep.resolution)) {
    return undefined
  }

  try {
    const indexFilePath = getIndexFilePath(storeDir, randomDep.resolution.integrity, randomDep.pkgId)
    const pkgIndex = readMsgpackFileSync<PackageFilesIndex>(indexFilePath)
    const manifest = readManifestFromIndex(storeDir, pkgIndex)

    if (!manifest) return undefined

    // Priority 1: funding
    const fundingUrl = normalizeFundingUrl(manifest.funding)
    if (fundingUrl) {
      return {
        packageName: randomDep.name,
        packageDescription: manifest.description,
        fundingUrl,
        fundingType: 'funding',
      }
    }

    // Priority 2: repository
    const repoUrl = normalizeRepositoryUrl(manifest.repository)
    if (repoUrl) {
      return {
        packageName: randomDep.name,
        packageDescription: manifest.description,
        fundingUrl: repoUrl,
        fundingType: 'repository',
      }
    }

    // Priority 3: homepage
    if (manifest.homepage) {
      return {
        packageName: randomDep.name,
        packageDescription: manifest.description,
        fundingUrl: manifest.homepage,
        fundingType: 'homepage',
      }
    }

    return undefined
  } catch {
    // If reading fails for any reason, just skip
    return undefined
  }
}
