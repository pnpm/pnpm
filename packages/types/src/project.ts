import { type ProjectManifest } from './package'

export interface Project {
  dir: string
  manifest: ProjectManifest
  writeProjectManifest: (manifest: ProjectManifest, force?: boolean | undefined) => Promise<void>
  modulesDir?: string
}

export type ProjectsGraph = Record<string, { dependencies: string[], package: Project }>
