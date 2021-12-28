export type Parents = Array<{ name: string, version: string }>

export interface MissingPeerDependencyIssue {
  parents: Parents
  optional: boolean
  wantedRange: string
}

export type MissingPeerIssuesByPeerName = Record<string, MissingPeerDependencyIssue[]>

export interface BadPeerDependencyIssue extends MissingPeerDependencyIssue {
  foundVersion: string
  resolvedFrom: Parents
}

export type BadPeerIssuesByPeerName = Record<string, BadPeerDependencyIssue[]>

export type PeerDependencyIssuesByProjects = Record<string, PeerDependencyIssues>

export interface PeerDependencyIssues {
  bad: BadPeerIssuesByPeerName
  missing: MissingPeerIssuesByPeerName
  conflicts: string[]
  intersections: Record<string, string>
}
