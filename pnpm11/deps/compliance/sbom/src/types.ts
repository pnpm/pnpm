import type { DepType } from '@pnpm/lockfile.detect-dep-types'

export interface SbomComponent {
  name: string
  version: string
  purl: string
  depPath: string
  depType: DepType
  integrity?: string
  tarballUrl?: string
  license?: string
  description?: string
  author?: string
  homepage?: string
  repository?: string
  bugsUrl?: string
}

export interface SbomRelationship {
  from: string
  to: string
}

export interface SbomRootComponent {
  name: string
  version: string
  type: 'library' | 'application'
  license?: string
  description?: string
  author?: string
  repository?: string
  bugsUrl?: string
}

export interface SbomResult {
  rootComponent: SbomRootComponent
  components: SbomComponent[]
  relationships: SbomRelationship[]
}

export type SbomFormat = 'cyclonedx' | 'spdx'
export type SbomComponentType = 'library' | 'application'
