import type { ProjectManifest, DevEngines, DevEngineDependency } from '@pnpm/types'
import { updateProjectManifest } from '@pnpm/read-project-manifest'

export const getNodeRuntime = ({ runtime }: DevEngines = {}): DevEngineDependency | undefined =>
  [runtime].flat().find(runtime => runtime?.name === 'node')

export async function updateNodeRuntimeVersion (projectDir: string, { devEngines }: ProjectManifest = {}, version: string): Promise<void> {
  if (!devEngines) return

  devEngines.runtime &&= Array.isArray(devEngines.runtime)
    ? devEngines.runtime?.map(runtime => runtime.name === 'node' ? { ...runtime, version } : runtime) ?? {}
    : { ...devEngines.runtime, version }

  return updateProjectManifest(projectDir, {devEngines})
}
