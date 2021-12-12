import { MergedPeersByProjects } from '@pnpm/types'
import { intersect } from 'semver-range-intersect'

export function mergePeersByProjects (missingPeersByProject: Record<string, Record<string, string[]>>): MergedPeersByProjects {
  const mergedPeersByProjects: MergedPeersByProjects = {}
  for (const [projectPath, rangesByPeerNames] of Object.entries(missingPeersByProject)) {
    mergedPeersByProjects[projectPath] = {
      conflicts: [],
      intersections: [],
    }
    for (const [peerName, ranges] of Object.entries(rangesByPeerNames)) {
      if (ranges.length === 1) {
        mergedPeersByProjects[projectPath].intersections.push({
          peerName,
          versionRange: ranges[0],
        })
        continue
      }
      const intersection = intersect(...ranges)
      if (intersection === null) {
        mergedPeersByProjects[projectPath].conflicts.push(peerName)
      } else {
        mergedPeersByProjects[projectPath].intersections.push({
          peerName,
          versionRange: intersection,
        })
      }
    }
  }
  return mergedPeersByProjects
}
