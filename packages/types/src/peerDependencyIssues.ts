export interface PeerDependencyIssueLocation {
  parents: Array<{ name: string, version: string }>
  projectPath: string
}

export interface MissingPeerDependencyIssue {
  location: PeerDependencyIssueLocation
  rootDir: string
  peerRange: string
}

export interface BadPeerDependencyIssue extends MissingPeerDependencyIssue {
  foundPeerVersion?: string
}

export interface PeerDependencyIssues {
  bad: Record<string, BadPeerDependencyIssue[]>
  missing: Record<string, MissingPeerDependencyIssue[]>
}
