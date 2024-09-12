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
  const hex = ssri.parse(integrity, { single: true }).hexDigest().substring(0, 12)
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
