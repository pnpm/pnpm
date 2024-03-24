import type { PackageFilesIndex } from '@pnpm/types'

export function readdir(index: PackageFilesIndex, dir: string): string[] {
  const dirs = new Set<string>()

  const prefix = dir ? `${dir}/` : ''

  for (const filePath of Object.keys(index.files)) {
    if (filePath.startsWith(prefix)) {
      const parts = filePath.substring(dir.length).split('/')

      const part = parts[0] ?? parts[1]

      if (part) {
        dirs.add(part)
      }
    }
  }

  return Array.from(dirs)
}

export function dirEntityType(index: PackageFilesIndex, p: string): 'file' | 'directory' | undefined {
  if (index.files[p]) {
    return 'file'
  }

  const prefix = `${p}/`

  return Object.keys(index.files).some((k) => k.startsWith(prefix))
    ? 'directory'
    : undefined
}
