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
