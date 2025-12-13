import { type PackageFilesIndex } from '@pnpm/store.cafs'

export function readdir (index: PackageFilesIndex, dir: string): string[] {
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

export function dirEntityType (index: PackageFilesIndex, p: string): DirEntityType | undefined {
  if (index.files.has(p)) return 'file'
  const prefix = `${p}/`
  return Array.from(index.files.keys()).some((k) => k.startsWith(prefix)) ? 'directory' : undefined
}
