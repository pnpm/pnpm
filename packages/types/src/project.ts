import { type ProjectManifest } from './package'

export interface Project {
  rootDir: string
  rootDirRealPath: string
  manifest: ProjectManifest
  writeProjectManifest: (manifest: ProjectManifest, force?: boolean | undefined) => Promise<void>
}

export type ProjectsGraph = Record<string, { dependencies: string[], package: Project }>
