import { getAllDependenciesFromPackage } from '@pnpm/utils'
import { readProjectManifest } from './readProjectManifest'

export async function readDepNameCompletions (dir?: string) {
  const { manifest } = await readProjectManifest(dir ?? process.cwd(), {})
  return Object.keys(
    getAllDependenciesFromPackage(manifest),
  ).map((name) => ({ name }))
}
