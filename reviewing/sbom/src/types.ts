import { type DepType } from '@pnpm/lockfile.detect-dep-types'

export interface SbomComponent {
  name: string
  version: string
  purl: string
  depPath: string
  depType: DepType
  integrity?: string
  license?: string
  description?: string
  author?: string
  homepage?: string
  repository?: string
}

export interface SbomRelationship {
  from: string
  to: string
}

export interface SbomResult {
  rootComponent: {
    name: string
    version: string
    type: 'library' | 'application'
  }
  components: SbomComponent[]
  relationships: SbomRelationship[]
}

export type SbomFormat = 'cyclonedx' | 'spdx'
export type SbomComponentType = 'library' | 'application'
