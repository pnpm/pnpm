import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import { readProjectManifest } from '@pnpm/workspace.read-project-manifest'

export async function readDepNameCompletions (dir?: string): Promise<Array<{ name: string }>> {
  const { manifest } = await readProjectManifest(dir ?? process.cwd())
  return Object.keys(
    getAllDependenciesFromManifest(manifest)
  ).map((name) => ({ name }))
}
