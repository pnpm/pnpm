import CI, { isCI } from 'ci-info'

let id = 0

export function logSectionStart (sectionName: string) {
  if (!isCI) return undefined
  const labels = getLabels(sectionName)
  if (labels) {
    process.stdout.write(labels.start)
    return () => {
      process.stdout.write(labels.end)
    }
  }
  return undefined
}

function getLabels (sectionName: string) {
  if (CI.GITHUB_ACTIONS) {
    return {
      start: `::group::${sectionName}\r\n`,
      end: '::endgroup::\r\n',
    }
  } else if (CI.GITLAB) {
    id++
    return {
      start: `section_start:${Math.floor(Date.now() / 1000)}:${id}\\r\\e[0K${sectionName}\r\n`,
      end: `section_end:${Math.floor(Date.now() / 1000)}:${id}\\r\\e[0K`,
    }
  } else if (CI.TRAVIS) {
    return {
      start: `travis_fold:start:${sectionName}\r\n`,
      end: `travis_fold:end:${sectionName}\r\n`,
    }
  } else if (CI.AZURE_PIPELINES) {
    return {
      start: `##[group]${sectionName}\r\n`,
      end: '##[endgroup]\r\n',
    }
  } else if (CI.BUILDKITE) {
    return {
      start: `--- ${sectionName}\r\n`,
      end: '\r\n',
    }
  }
  return null
}
