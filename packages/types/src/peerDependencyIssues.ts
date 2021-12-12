export interface PeerDependencyIssueLocation {
  parents: Array<{ name: string, version: string }>
  projectId: string
}

export interface MissingPeerDependencyIssue {
  location: PeerDependencyIssueLocation
  wantedRange: string
}

export type MissingPeerIssuesByPeerName = Record<string, MissingPeerDependencyIssue[]>

export interface BadPeerDependencyIssue extends MissingPeerDependencyIssue {
  foundVersion: string
}

export type BadPeerIssuesByPeerName = Record<string, BadPeerDependencyIssue[]>

export interface PeerDependencyIssues {
  bad: BadPeerIssuesByPeerName
  missing: MissingPeerIssuesByPeerName
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
