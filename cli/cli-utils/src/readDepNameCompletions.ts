import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { readProjectManifest } from '@pnpm/read-project-manifest'

export async function readDepNameCompletions(dir?: string | undefined): Promise<
  {
    name: string
  }[]
> {
  const { manifest } = await readProjectManifest(dir ?? process.cwd())
  return Object.keys(getAllDependenciesFromManifest(manifest)).map(
    (
      name: string
    ): {
      name: string
    } => {
      return {
        name,
      }
    }
  )
}
