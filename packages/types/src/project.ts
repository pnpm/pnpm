import { type ProjectManifest } from './package'

export interface Project {
  rootDir: ProjectRootDir
  rootDirRealPath: ProjectRootDirRealPath
  manifest: ProjectManifest
  writeProjectManifest: (manifest: ProjectManifest, force?: boolean | undefined) => Promise<void>
}

export type ProjectsGraph = Record<ProjectRootDir, { dependencies: ProjectRootDir[], package: Project }>

export type ProjectRootDir = string & { __brand: 'ProjectRootDir' }

export type ProjectRootDirRealPath = string & { __brand: 'ProjectRootDirRealPath' }
