import { ProjectManifest } from './package'

export type Project = {
  dir: string,
  manifest: ProjectManifest,
  writeProjectManifest (manifest: ProjectManifest, force?: boolean | undefined): Promise<void>,
}

export type ProjectsGraph = Record<string, { dependencies: string[], package: Project }>
