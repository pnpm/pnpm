import { readPackageJson } from '@pnpm/read-package-json'
import { parse } from '@pnpm/dependency-path'
import { readMsgpackFile } from '@pnpm/fs.msgpack-file'
import { type PackageManifest, type Registries } from '@pnpm/types'
import {
  getFilePathByModeInCafs,
  getIndexFilePathInCafs,
  type PackageFileInfo,
  type PackageFilesIndex,
} from '@pnpm/store.cafs'
import { type PackageSnapshot, pkgSnapshotToResolution, type Resolution, type DirectoryResolution } from '@pnpm/lockfile.utils'
import { fetchFromDir } from '@pnpm/directory-fetcher'
import { depPathToFilename } from '@pnpm/dependency-path'
import pLimit from 'p-limit'
import path from 'path'

const limitMetadataReads = pLimit(4)

export interface PkgMetadata {
  license?: string
  description?: string
  author?: string
  homepage?: string
  repository?: string
}

export interface GetPkgMetadataOptions {
  storeDir: string
  lockfileDir: string
  virtualStoreDirMaxLength: number
}

export async function getPkgMetadata (
  depPath: string,
  snapshot: PackageSnapshot,
  registries: Registries,
  opts: GetPkgMetadataOptions
): Promise<PkgMetadata> {
  return limitMetadataReads(() => getPkgMetadataUnclamped(depPath, snapshot, registries, opts))
}

async function getPkgMetadataUnclamped (
  depPath: string,
  snapshot: PackageSnapshot,
  registries: Registries,
  opts: GetPkgMetadataOptions
): Promise<PkgMetadata> {
  const id = snapshot.id ?? depPath
  const resolution = pkgSnapshotToResolution(depPath, snapshot, registries) as Resolution

  let manifestPath: string

  if (resolution.type === 'directory') {
    const localInfo = await fetchFromDir(
      path.join(opts.lockfileDir, (resolution as DirectoryResolution).directory),
      {}
    )
    manifestPath = localInfo.filesMap.get('package.json') ?? ''
  } else {
    const isPackageWithIntegrity = 'integrity' in resolution

    let pkgIndexFilePath: string
    if (isPackageWithIntegrity) {
      const parsedId = parse(id)
      pkgIndexFilePath = getIndexFilePathInCafs(
        opts.storeDir,
        resolution.integrity as string,
        parsedId.nonSemverVersion ?? `${parsedId.name}@${parsedId.version}`
      )
    } else if (!resolution.type && 'tarball' in resolution && resolution.tarball) {
      const packageDirInStore = depPathToFilename(parse(id).nonSemverVersion ?? id, opts.virtualStoreDirMaxLength)
      pkgIndexFilePath = path.join(
        opts.storeDir,
        packageDirInStore,
        'integrity.mpk'
      )
    } else {
      return {}
    }

    try {
      const { files } = await readMsgpackFile<PackageFilesIndex>(pkgIndexFilePath)
      const pkgJsonInfo = files.get('package.json') as PackageFileInfo | undefined
      if (!pkgJsonInfo) return {}
      manifestPath = getFilePathByModeInCafs(opts.storeDir, pkgJsonInfo.digest, pkgJsonInfo.mode)
    } catch {
      return {}
    }
  }

  if (!manifestPath) return {}

  let manifest: PackageManifest | undefined
  try {
    manifest = await readPackageJson(manifestPath)
  } catch {
    return {}
  }

  if (!manifest) return {}

  return extractMetadata(manifest)
}

function extractMetadata (manifest: PackageManifest): PkgMetadata {
  return {
    license: parseLicenseField(manifest.license),
    description: manifest.description,
    author: parseAuthorField(manifest.author),
    homepage: manifest.homepage,
    repository: parseRepositoryField(manifest.repository),
  }
}

function parseLicenseField (field: unknown): string | undefined {
  if (typeof field === 'string') return field
  if (field && typeof field === 'object' && 'type' in field) {
    return (field as { type: string }).type
  }
  if (Array.isArray(field)) {
    return field
      .map((l: { type?: string }) => l.type)
      .filter(Boolean)
      .join(' OR ') || undefined
  }
  return undefined
}

function parseAuthorField (field: unknown): string | undefined {
  if (!field) return undefined
  if (typeof field === 'string') return field
  if (typeof field === 'object' && 'name' in field) {
    return (field as { name: string }).name
  }
  return undefined
}

function parseRepositoryField (field: unknown): string | undefined {
  if (!field) return undefined
  if (typeof field === 'string') return field
  if (typeof field === 'object' && 'url' in field) {
    return (field as { url: string }).url
  }
  return undefined
}
