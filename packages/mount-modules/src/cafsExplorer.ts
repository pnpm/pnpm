import { PackageFilesIndex } from '@pnpm/cafs'

export function readdir (index: PackageFilesIndex, dir: string) {
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

export function dirEntityType (index: PackageFilesIndex, p: string) {
  if (index.files[p]) return 'file'
  const prefix = `${p}/`
  return Object.keys(index.files).some((k) => k.startsWith(prefix)) ? 'directory' : undefined
}
