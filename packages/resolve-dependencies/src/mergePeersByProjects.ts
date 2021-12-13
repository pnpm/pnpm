import { MergedPeersByProjects } from '@pnpm/types'
import { intersect } from 'semver-range-intersect'

export type MissingPeersByProject = Record<string, Record<string, Array<{ range: string, optional: boolean }>>>

export function mergePeersByProjects (missingPeersByProject: MissingPeersByProject): MergedPeersByProjects {
  const mergedPeersByProjects: MergedPeersByProjects = {}
  for (const [projectPath, rangesByPeerNames] of Object.entries(missingPeersByProject)) {
    mergedPeersByProjects[projectPath] = {
      conflicts: [],
      intersections: {},
    }
    for (const [peerName, ranges] of Object.entries(rangesByPeerNames)) {
      if (ranges.every(({ optional }) => optional)) continue
      if (ranges.length === 1) {
        mergedPeersByProjects[projectPath].intersections[peerName] = ranges[0].range
        continue
      }
      const intersection = intersect(...ranges.map(({ range }) => range))
      if (intersection === null) {
        mergedPeersByProjects[projectPath].conflicts.push(peerName)
      } else {
        mergedPeersByProjects[projectPath].intersections[peerName] = intersection
      }
    }
  }
  return mergedPeersByProjects
}
