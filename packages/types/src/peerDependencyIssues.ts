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
}
