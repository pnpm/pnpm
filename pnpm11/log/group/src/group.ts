import CI, { isCI } from 'ci-info'

// GitLab hides a section marker only when it is wrapped in these bytes:
// https://docs.gitlab.com/ci/jobs/job_logs/
const ERASE_LINE = '\x1b[0K'

let id = 0

export function groupStart (groupName: string): (() => void) | undefined {
  if (!isCI) return undefined
  const labels = getLabels(groupName)
  if (labels) {
    process.stdout.write(labels.start)
    return () => {
      process.stdout.write(labels.end)
    }
  }
  return undefined
}

function getLabels (groupName: string) {
  if (CI.GITHUB_ACTIONS) {
    return {
      start: `::group::${groupName}\r\n`,
      end: '::endgroup::\r\n',
    }
  } else if (CI.GITLAB) {
    id++
    return {
      start: `${ERASE_LINE}section_start:${Math.floor(Date.now() / 1000)}:${id}\r${ERASE_LINE}${groupName}\r\n`,
      end: `${ERASE_LINE}section_end:${Math.floor(Date.now() / 1000)}:${id}\r${ERASE_LINE}\r\n`,
    }
  } else if (CI.TRAVIS) {
    return {
      start: `travis_fold:start:${groupName}\r\n`,
      end: `travis_fold:end:${groupName}\r\n`,
    }
  } else if (CI.AZURE_PIPELINES) {
    return {
      start: `##[group]${groupName}\r\n`,
      end: '##[endgroup]\r\n',
    }
  } else if (CI.BUILDKITE) {
    return {
      start: `--- ${groupName}\r\n`,
      end: '\r\n',
    }
  }
  return null
}
