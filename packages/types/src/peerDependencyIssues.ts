export interface PeerDependencyIssueLocation {
  parents: Array<{ name: string, version: string }>
  projectPath: string
}

export interface MissingPeerDependencyIssue {
  location: PeerDependencyIssueLocation
  wantedRange: string
}

export interface BadPeerDependencyIssue extends MissingPeerDependencyIssue {
  foundVersion: string
}

export interface PeerDependencyIssues {
  bad: Record<string, BadPeerDependencyIssue[]>
  missing: Record<string, MissingPeerDependencyIssue[]>
  missingMergedByProjects: MergedPeersByProjects
}

export type MergedPeersByProjects = Record<string, MergedPeers>

export interface MergedPeers {
  conflicts: string[]
  intersections: PeerIntersection[]
}

export interface PeerIntersection {
  peerName: string
  versionRange: string
}
