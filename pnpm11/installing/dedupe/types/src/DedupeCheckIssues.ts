export interface DedupeCheckIssues {
  readonly importerIssuesByImporterId: SnapshotsChanges
  readonly packageIssuesByDepPath: SnapshotsChanges
}

export interface SnapshotsChanges {
  readonly added: readonly string[]
  readonly removed: readonly string[]
  readonly updated: Record<string, ResolutionChangesByAlias>
}

export type ResolutionChangesByAlias = Record<string, ResolutionChange>

export type ResolutionChange = ResolutionAdded | ResolutionDeleted | ResolutionUpdated

export interface ResolutionAdded {
  readonly type: 'added'
  readonly next: string
}

export interface ResolutionDeleted {
  readonly type: 'removed'
  readonly prev: string
}

export interface ResolutionUpdated {
  readonly type: 'updated'
  readonly prev: string
  readonly next: string
}
