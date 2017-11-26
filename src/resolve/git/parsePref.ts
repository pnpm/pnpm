import HostedGit = require('hosted-git-info')
import url = require('url')

export type HostedPackageSpec = ({
  fetchSpec: string,
  hosted?: {
    type: string,
    shortcut: string,
    sshUrl: string,
    user: string,
    project: string,
    committish: string,
  },
  normalizedPref: string,
} & ({
  gitCommittish: string | null,
} | {
  gitCommittish: null,
  gitRange: string,
}))

const gitProtocols = new Set([
  'git',
  'git+http',
  'git+https',
  'git+rsync',
  'git+ftp',
  'git+file',
  'git+ssh',
])

export default function parsePref (pref: string, alias?: string): HostedPackageSpec | null {
  const hosted = HostedGit.fromUrl(pref, {noGitPlus: true, noCommittish: true})
  if (hosted) {
    return fromHostedGit(hosted)
  }
  const colonsPos = pref.indexOf(':')
  if (colonsPos === -1) return null
  const protocol = pref.substr(0, colonsPos)
  if (protocol && gitProtocols.has(protocol.toLocaleLowerCase())) {
    const urlparse = url.parse(pref)
    if (!urlparse || !urlparse.protocol) return null
    const match = urlparse.protocol === 'git+ssh:' && matchGitScp(pref)
    if (match) {
      return {
        ...match,
        normalizedPref: pref,
      }
    }
    return {
      fetchSpec: urlToFetchSpec(urlparse),
      normalizedPref: pref,
      ...setGitCommittish(urlparse.hash != null ? urlparse.hash.slice(1) : ''),
    }
  }
  return null
}

function urlToFetchSpec (urlparse: url.Url) {
  if (urlparse.protocol) {
    urlparse.protocol = urlparse.protocol.replace(/^git[+]/, '')
  }
  delete urlparse.hash
  return url.format(urlparse)
}

function fromHostedGit (hosted: any): HostedPackageSpec { // tslint:disable-line
  return {
    fetchSpec: hosted.getDefaultRepresentation() === 'shortcut' ? null : hosted.toString(),
    hosted,
    normalizedPref: hosted.toString({noGitPlus: false, noCommittish: false}),
    ...setGitCommittish(hosted.committish),
  }
}

function setGitCommittish (committish: string | null) {
  if (committish != null && committish.length >= 7 && committish.slice(0, 7) === 'semver:') {
    return {
      gitCommittish: null,
      gitRange: decodeURIComponent(committish.slice(7)),
    }
  }
  return {
    gitCommittish: committish === '' ? null : committish,
  }
}

function matchGitScp (spec: string) {
  // git ssh specifiers are overloaded to also use scp-style git
  // specifiers, so we have to parse those out and treat them special.
  // They are NOT true URIs, so we can't hand them to `url.parse`.
  //
  // This regex looks for things that look like:
  // git+ssh://git@my.custom.git.com:username/project.git#deadbeef
  //
  // ...and various combinations. The username in the beginning is *required*.
  const matched = spec.match(/^git\+ssh:\/\/([^:#]+:[^#]+(?:\.git)?)(?:#(.*))?$/i)
  return matched && !matched[1].match(/:[0-9]+\/?.*$/i) && {
    fetchSpec: matched[1],
    gitCommittish: matched[2] == null ? null : matched[2],
  }
}
