import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { indexOfDepPathSuffix } from '@pnpm/deps.path'

const LOCAL_TARBALL_EXTENSIONS = [
  '.tgz',
  '.tar.gz',
  '.tar',
  '.tar.bz2',
  '.tbz2',
  '.tbz',
]

export function refIsLocalTarball (ref: string): boolean {
  if (!ref.startsWith('file:')) return false
  const { peersIndex } = indexOfDepPathSuffix(ref)
  const cleanRef = peersIndex === -1 ? ref : ref.slice(0, peersIndex)
  const lower = cleanRef.toLowerCase()
  return LOCAL_TARBALL_EXTENSIONS.some((ext) => lower.endsWith(ext))
}

export function refIsLocalDirectory (ref: string): boolean {
  return ref.startsWith('file:') && !refIsLocalTarball(ref)
}

function isUncLikeFilePayload (pathPart: string): boolean {
  return pathPart.startsWith('\\\\') ||
    pathPart.startsWith('////') ||
    (pathPart.startsWith('//') && !pathPart.startsWith('///'))
}

export function resolveLocalTarballPath (lockfileDir: string, tarball: string): string | undefined {
  if (!refIsLocalTarball(tarball)) return undefined
  const pathPart = tarball.slice('file:'.length)
  if (isUncLikeFilePayload(pathPart) || pathPart.includes('\0')) {
    return undefined
  }
  if (pathPart.startsWith('/') && tarball.startsWith('file:///')) {
    return localFileUrlToPath(tarball)
  }
  if (path.isAbsolute(pathPart)) {
    return path.normalize(pathPart)
  }
  return path.resolve(lockfileDir, pathPart)
}

function localFileUrlToPath (tarball: string): string | undefined {
  try {
    const url = new URL(tarball)
    if (url.protocol !== 'file:' || url.host) {
      return undefined
    }
    return fileURLToPath(url)
  } catch {
    return undefined
  }
}
