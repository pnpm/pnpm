import { type PackageFilesRaw } from '@pnpm/store.cafs'

export function readdir (index: { files: PackageFilesRaw }, dir: string): string[] {
  const dirs = new Set<string>()
  const prefix = dir ? `${dir}/` : ''
  for (const filePath of Object.keys(index.files)) {
    if (filePath.startsWith(prefix)) {
      const parts = filePath.substring(dir.length).split('/')
      dirs.add(parts[0] || parts[1])
    }
  }
  return Array.from(dirs)
}

export type DirEntityType = 'file' | 'directory'

export function dirEntityType (index: { files: PackageFilesRaw }, p: string): DirEntityType | undefined {
  if (index.files[p]) return 'file'
  const prefix = `${p}/`
  return Object.keys(index.files).some((k) => k.startsWith(prefix)) ? 'directory' : undefined
}
