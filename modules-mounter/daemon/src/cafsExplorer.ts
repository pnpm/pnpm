import type { PackageFiles } from '@pnpm/store.cafs'

export function readdir (index: { files: PackageFiles }, dir: string): string[] {
  const dirs = new Set<string>()
  const prefix = dir ? `${dir}/` : ''
  for (const filePath of index.files.keys()) {
    if (filePath.startsWith(prefix)) {
      const parts = filePath.substring(dir.length).split('/')
      dirs.add(parts[0] || parts[1])
    }
  }
  return Array.from(dirs)
}

export type DirEntityType = 'file' | 'directory'

export function dirEntityType (index: { files: PackageFiles }, p: string): DirEntityType | undefined {
  if (index.files.has(p)) return 'file'
  const prefix = `${p}/`
  for (const k of index.files.keys()) {
    if (k.startsWith(prefix)) return 'directory'
  }
  return undefined
}
