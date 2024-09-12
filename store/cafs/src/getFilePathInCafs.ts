import path from 'path'
import ssri, { type IntegrityLike } from 'ssri'

export const modeIsExecutable = (mode: number): boolean => (mode & 0o111) === 0o111

export type FileType = 'exec' | 'nonexec' | 'index'

export function getFilePathByModeInCafs (
  cafsDir: string,
  integrity: string | IntegrityLike,
  mode: number
): string {
  const fileType = modeIsExecutable(mode) ? 'exec' : 'nonexec'
  return path.join(cafsDir, contentPathFromIntegrity(integrity, fileType))
}

export function getIndexFilePathInCafs (
  cafsDir: string,
  integrity: string | IntegrityLike,
  pkgId: string
): string {
  const hex = ssri.parse(integrity, { single: true }).hexDigest().substring(0, 64)
  // Some registries allow identical content to be published under different package names or versions.
  // To accommodate this, index files are stored using both the content hash and package identifier.
  // This approach ensures that we can:
  // 1. Validate that the integrity in the lockfile corresponds to the correct package,
  //    which might not be the case after a poorly resolved Git conflict.
  // 2. Allow the same content to be referenced by different packages or different versions of the same package.
  return path.join(cafsDir, `${path.join(hex.slice(0, 2), hex.slice(2))}-${pkgId.replace(/[\\/:*?"<>|]/g, '+')}.json`)
}

function contentPathFromIntegrity (
  integrity: string | IntegrityLike,
  fileType: FileType
): string {
  const sri = ssri.parse(integrity, { single: true })
  return contentPathFromHex(fileType, sri.hexDigest())
}

export function contentPathFromHex (fileType: FileType, hex: string): string {
  const p = path.join(hex.slice(0, 2), hex.slice(2))
  switch (fileType) {
  case 'exec':
    return `${p}-exec`
  case 'nonexec':
    return p
  case 'index':
    return `${p}-index.json`
  }
}
