import { promises as fs } from 'fs'
import path from 'path'
import { Cafs, FilesIndex, DeferredManifestPromise } from '@pnpm/fetcher-base'
import loadJsonFile from 'load-json-file'

export default () => {
  return {
    directory: async function fetchFromLocal (
      cafs: Cafs,
      resolution: {
        directory: string
        type: 'directory'
      },
      opts: {
        manifest?: DeferredManifestPromise
      }
    ) {
      const filesIndex: FilesIndex = {}
      await mapDirectory(resolution.directory, resolution.directory, filesIndex)
      if (opts.manifest) {
        opts.manifest.resolve(await loadJsonFile(path.join(resolution.directory, 'package.json')))
      }
      return {
        filesIndex,
        packageImportMethod: 'hardlink',
      }
    },
  }
}

async function mapDirectory (
  rootDir: string,
  currDir: string,
  index: FilesIndex
) {
  const files = await fs.readdir(currDir)
  await Promise.all(files.filter((file) => file !== 'node_modules').map(async (file) => {
    const fullPath = path.join(currDir, file)
    const stat = await fs.stat(fullPath)
    if (stat.isDirectory()) {
      await mapDirectory(rootDir, fullPath, index)
      return
    }
    if (stat.isFile()) {
      const relativePath = path.relative(rootDir, fullPath)
      index[relativePath] = {
        location: fullPath,
      } as any // eslint-disable-line
    }
  }))
}
