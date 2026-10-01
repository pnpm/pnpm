import type semver from 'semver'

export interface OutdatedGitHubAction {
  current: string
  latest: string
  name: string
  wanted: string
  homepage: string
}

export interface GitHubActionsOptions {
  dir: string
  match?: (name: string) => boolean
  readRepoRefs?: (repo: string) => Promise<Record<string, string>>
  /**
   * Reads the creation dates of tags of a repository. Defaults to fetching
   * the tags without their trees.
   */
  readTagDates?: (repo: string, tags: string[]) => Promise<Record<string, Date>>
  /**
   * Versions tagged fewer than this many minutes ago are not offered.
   */
  minimumReleaseAge?: number
  /**
   * Actions exempt from `minimumReleaseAge`, matched against the action and
   * repository names.
   */
  minimumReleaseAgeExclude?: string[]
  /**
   * The base URL of the GitHub server hosting the action repositories.
   * Defaults to the `GITHUB_SERVER_URL` environment variable, or
   * https://github.com.
   */
  serverUrl?: string
}

export interface FindOutdatedGitHubActionsOptions extends GitHubActionsOptions {
  compatible?: boolean
}

export interface UpdateGitHubActionsOptions extends GitHubActionsOptions {
  latest?: boolean
}

export interface GitHubActionsOptInOptions {
  includeGithubActions?: boolean
  updateConfig?: { githubActions?: boolean }
}

export interface ActionReference {
  commentVersion?: string
  file: ActionFile
  flowStyle: boolean
  indentation: string
  name: string
  originalValue: string
  range: readonly [number, number]
  ref: string
  repo: string
}

export interface ActionFile {
  path: string
}

export interface RepoVersion {
  commit: string
  tag: string
  version: semver.SemVer
}

export interface PlannedUpdate {
  action: ActionReference
  current: RepoVersion
  latest: RepoVersion
  wanted: RepoVersion
}
