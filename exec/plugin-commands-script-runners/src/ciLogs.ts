import CI, { isCI } from 'ci-info'

export class CiLogs {
  constructor (private readonly opts: {
    concurrency?: number
    script?: string
    name?: string
    version?: string
    prefix: string
  }) {
    if (!this.opts.concurrency) {
      this.opts.concurrency = 4
    }
  }

  private labels () {
    const { name, version, prefix, script } = this.opts
    const ScriptName = `${name ?? 'unknown'}${version ? `@${version}` : ''} ${script ? `: ${script}` : ''} ${prefix}`
    const id = `${name ?? 'unknown'}@${version ?? 'unknown'}-${script}`

    if (CI.GITHUB_ACTIONS) {
      return {
        start: `::group::${ScriptName}\r\n`,
        end: '::endgroup::\r\n',
      }
    } else if (CI.GITLAB) {
      return {
        start: `section_start:${Math.floor(Date.now() / 1000)}:${id}\\r\\e[0K${ScriptName}\r\n`,
        end: `section_end:${Math.floor(Date.now() / 1000)}:${id}\\r\\e[0K`,
      }
    } else if (CI.TRAVIS) {
      return {
        start: `travis_fold:start:${ScriptName}\r\n`,
        end: `travis_fold:end:${ScriptName}\r\n`,
      }
    } else if (CI.AZURE_PIPELINES) {
      return {
        start: `##[group]${ScriptName}\r\n`,
        end: '##[endgroup]\r\n',
      }
    } else if (CI.BUILDKITE) {
      return {
        start: `--- ${ScriptName}\r\n`,
        end: '\r\n',
      }
    }

    return null
  }

  /**
   * Logs the start or end of a script run if the current environment is a CI.
   * @param type - Either 'start' or 'end'.
   */
  public log (type: 'start' | 'end') {
    if (!isCI || this.opts.concurrency! > 1) return
    const labels = this.labels()
    if (labels) {
      process.stdout.write(labels[type])
    }
  }
}