export interface MissingPeerDependencyIssue {
  parents: Array<{ name: string, version: string }>
  optional: boolean
  wantedRange: string
}

export type MissingPeerIssuesByPeerName = Record<string, MissingPeerDependencyIssue[]>

export interface BadPeerDependencyIssue extends MissingPeerDependencyIssue {
  foundVersion: string
}

export type BadPeerIssuesByPeerName = Record<string, BadPeerDependencyIssue[]>

export type PeerDependencyIssues = Record<string, PeerDependencyIssuesOfProject>

export interface PeerDependencyIssuesOfProject {
  bad: BadPeerIssuesByPeerName
  missing: MissingPeerIssuesByPeerName
  conflicts: string[]
  intersections: Record<string, string>
}
