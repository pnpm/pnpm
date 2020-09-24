import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import readProjectManifest from '@pnpm/read-project-manifest'

export async function readDepNameCompletions (dir?: string) {
  const { manifest } = await readProjectManifest(dir ?? process.cwd())
  return Object.keys(
    getAllDependenciesFromManifest(manifest)
  ).map((name) => ({ name }))
}
