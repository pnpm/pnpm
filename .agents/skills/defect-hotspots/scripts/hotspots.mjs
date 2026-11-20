#!/usr/bin/env node
// Collects the evidence the defect-hotspots skill analyzes: fix temperature per
// file and module, fix-on-recent-change edges (SZZ blame), and recent issues
// linked to the modules their fixes touched. Writes hotspots.json and prints a
// Markdown summary.
import { execFile, execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { parseArgs, promisify } from 'node:util'

const execFileAsync = promisify(execFile)
const DAY_MS = 24 * 60 * 60 * 1000
const COMMIT_SEPARATOR = '\x1e'
const FIELD_SEPARATOR = '\x1f'
const FILES_SEPARATOR = '\x1d'
const BLAME_CONCURRENCY = 8
// A commit touching more source files than this is treated as mechanical (a
// move, a split, a rename) and blame looks through it.
const MECHANICAL_FILE_COUNT = 30

const { values: options } = parseArgs({
  options: {
    ref: { type: 'string', default: 'origin/main' },
    'history-days': { type: 'string', default: '90' },
    'recent-days': { type: 'string', default: '21' },
    'issue-days': { type: 'string', default: '21' },
    'max-intro-age-days': { type: 'string', default: '60' },
    out: { type: 'string', default: '.' },
    'no-issues': { type: 'boolean', default: false },
    top: { type: 'string', default: '25' },
    help: { type: 'boolean', default: false },
  },
})

if (options.help) {
  process.stdout.write(`Usage: node hotspots.mjs [options]

  --ref <rev>                  history to analyze (default origin/main)
  --history-days <n>           commits to read (default 90)
  --recent-days <n>            window whose fixes are blamed (default 21)
  --max-intro-age-days <n>     a blamed commit younger than this counts as a
                               fix-on-recent-change edge (default 60)
  --issue-days <n>             issues created within this window (default 21)
  --no-issues                  skip the GitHub issue fetch
  --top <n>                    rows per summary table (default 25)
  --out <dir>                  where hotspots.json is written (default .)\n`)
  process.exit(0)
}

const historyDays = Number(options['history-days'])
const recentDays = Number(options['recent-days'])
const issueDays = Number(options['issue-days'])
const maxIntroAgeDays = Number(options['max-intro-age-days'])
const topCount = Number(options.top)
const now = Date.now()
if (execFileSync('git', ['rev-parse', '--is-shallow-repository'], { encoding: 'utf8' }).trim() === 'true') {
  process.stderr.write('The checkout is shallow, so history stops at its boundary. Run `git fetch --unshallow origin` first.\n')
  process.exit(1)
}
const headSha = execFileSync('git', ['rev-parse', '--verify', '--end-of-options', `${options.ref}^{commit}`], { encoding: 'utf8' }).trim()

const loadedCommits = readCommits(headSha, Math.max(historyDays, recentDays + maxIntroAgeDays))
const commits = loadedCommits.filter(commit => now - commit.time <= historyDays * DAY_MS)
const bySha = new Map(loadedCommits.map(commit => [commit.sha, commit]))
const mechanicalShas = loadedCommits.filter(commit => commit.sourceFiles.length > MECHANICAL_FILE_COUNT).map(commit => commit.sha)
const mechanicalShaSet = new Set(mechanicalShas)
const lineCounts = countLines(headSha)
const recentFixes = commits.filter(commit => commit.kind === 'fix' && now - commit.time <= recentDays * DAY_MS)
const edges = await blameFixes(recentFixes)
const issues = options['no-issues'] ? [] : fetchIssues(issueDays)
linkIssues(issues, commits)
const files = summarizeFiles(commits)
const modules = summarizeModules(commits, edges, issues)
const introducers = summarizeIntroducers(edges)

const report = {
  generatedAt: new Date(now).toISOString(),
  ref: options.ref,
  windows: { historyDays, recentDays, issueDays, maxIntroAgeDays },
  totals: {
    mechanicalCommits: mechanicalShas.length,
    commits: commits.length,
    fixes: commits.filter(commit => commit.kind === 'fix').length,
    recentFixes: recentFixes.length,
    reverts: commits.filter(commit => commit.kind === 'revert').length,
    issues: issues.length,
  },
  modules,
  files,
  introducers,
  edges,
  reverts: commits.filter(commit => commit.kind === 'revert').map(pickCommitFields),
  issues,
}
fs.mkdirSync(options.out, { recursive: true })
const jsonPath = path.join(options.out, 'hotspots.json')
fs.writeFileSync(jsonPath, JSON.stringify(report, null, 2))
printSummary(report, jsonPath)

function readCommits (ref, days) {
  const raw = execFileSync('git', [
    'log', '--no-merges', `--since=${days} days ago`,
    `--format=${COMMIT_SEPARATOR}%H${FIELD_SEPARATOR}%ct${FIELD_SEPARATOR}%s${FIELD_SEPARATOR}%b${FILES_SEPARATOR}`,
    '--name-only', ref,
  ], { encoding: 'utf8', maxBuffer: 1024 * 1024 * 1024 })
  return raw.split(COMMIT_SEPARATOR).filter(Boolean).map(parseCommit)
}

function parseCommit (record) {
  const [header, fileList] = record.split(FILES_SEPARATOR)
  const [sha, timestamp, subject, body] = header.split(FIELD_SEPARATOR)
  const paths = fileList.split('\n').map(line => line.trim()).filter(Boolean)
  const message = `${subject}\n${body}`
  return {
    sha,
    time: Number(timestamp) * 1000,
    subject,
    kind: classifyCommit(subject),
    pr: subject.match(/\(#(\d+)\)$/)?.[1],
    issueRefs: closedIssues(message),
    sourceFiles: paths.filter(file => classifyPath(file) === 'source'),
    testFiles: paths.filter(file => classifyPath(file) === 'test'),
  }
}

function closedIssues (message) {
  const refs = new Set()
  for (const clause of message.matchAll(/\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?):?\s+((?:(?:pnpm\/pnpm#|#|https:\/\/github\.com\/pnpm\/pnpm\/issues\/)\d+(?:,\s*|\s+and\s+)?)+)/gi)) {
    for (const ref of clause[1].matchAll(/(?:#|issues\/)(\d+)/g)) refs.add(ref[1])
  }
  return [...refs]
}

function classifyCommit (subject) {
  if (/^revert/i.test(subject)) return 'revert'
  return subject.match(/^(\w+)(?:\([^)]*\))?!?:/)?.[1] ?? 'other'
}

function classifyPath (file) {
  if (
    file.startsWith('.changeset/') ||
    /(?:^|\/)docs\//.test(file) ||
    /\.(?:md|mdx|snap|lock|ya?ml|json|toml)$/.test(file) ||
    file.includes('__snapshots__/') ||
    file.includes('/snapshots/')
  ) return 'ignored'
  if (
    /(?:^|\/)(?:tests?|__tests__|fixtures|__fixtures__|testing)\//.test(file) ||
    /\.(?:test|spec)\.[cm]?[jt]sx?$/.test(file) ||
    /(?:^|[/_])tests?\.rs$/.test(file)
  ) return 'test'
  if (/\.(?:rs|[cm]?[jt]sx?|sh|ps1)$/.test(file)) return 'source'
  return 'ignored'
}

function countLines (ref) {
  const raw = execFileSync('git', ['grep', '-I', '-c', '', ref], { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 })
  const counts = new Map()
  for (const line of raw.split('\n')) {
    const separator = line.lastIndexOf(':')
    if (separator === -1) continue
    const file = line.slice(ref.length + 1, separator)
    if (classifyPath(file) === 'source') counts.set(file, Number(line.slice(separator + 1)))
  }
  return counts
}

function moduleOf (file) {
  const segments = file.split('/')
  if ((segments[0] === 'pnpm' || segments[0] === 'pnpr') && segments[1] === 'crates') {
    return segments.slice(0, 3).join('/')
  }
  if (segments[1] === 'npm' || segments[0] === 'pnpm11') {
    return segments.slice(0, segments[0] === 'pnpm11' && segments[1] === 'pnpm' ? 2 : 3).join('/')
  }
  return segments.slice(0, 2).join('/')
}

async function blameFixes (fixes) {
  const jobs = []
  for (const fix of fixes) {
    for (const file of fix.sourceFiles) jobs.push({ fix, file })
  }
  const results = await mapWithConcurrency(jobs, BLAME_CONCURRENCY, blameJob)
  return results.flat()
}

async function blameJob ({ fix, file }) {
  const ranges = await removedRanges(fix.sha, file)
  if (ranges.length === 0) return []
  const args = ['blame', '--porcelain', '-w', '-M', '-C', ...mechanicalShas.flatMap(sha => ['--ignore-rev', sha]), ...ranges.flatMap(([start, count]) => ['-L', `${start},+${count}`]), `${fix.sha}^`, '--', file]
  let output
  try {
    ({ stdout: output } = await execFileAsync('git', args, { maxBuffer: 256 * 1024 * 1024 }))
  } catch {
    return []
  }
  const linesBySha = new Map()
  for (const line of output.split('\n')) {
    const sha = line.match(/^([0-9a-f]{40}) \d+ \d+/)?.[1]
    if (sha) linesBySha.set(sha, (linesBySha.get(sha) ?? 0) + 1)
  }
  const edges = []
  for (const [introSha, lines] of linesBySha) {
    const intro = bySha.get(introSha)
    // Blame cannot look through a split that moved lines into a new file.
    if (!intro || intro.sha === fix.sha || mechanicalShaSet.has(introSha)) continue
    const ageDays = (fix.time - intro.time) / DAY_MS
    if (ageDays > maxIntroAgeDays) continue
    edges.push({
      fix: pickCommitFields(fix),
      introducer: pickCommitFields(intro),
      file,
      module: moduleOf(file),
      lines,
      ageDays: Math.round(ageDays * 10) / 10,
    })
  }
  return edges
}

async function removedRanges (sha, file) {
  const { stdout } = await execFileAsync('git', ['show', '-U0', '--format=', sha, '--', file], { maxBuffer: 256 * 1024 * 1024 })
  const ranges = []
  for (const match of stdout.matchAll(/^@@ -(\d+)(?:,(\d+))? \+/gm)) {
    const start = Number(match[1])
    const count = match[2] === undefined ? 1 : Number(match[2])
    if (count > 0) ranges.push([start, count])
  }
  return ranges
}

function fetchIssues (days) {
  const since = new Date(now - days * DAY_MS).toISOString().slice(0, 10)
  const raw = execFileSync('gh', [
    'issue', 'list', '-R', 'pnpm/pnpm', '--state', 'all', '--limit', '1000',
    '--search', `created:>=${since}`,
    '--json', 'number,title,labels,state,stateReason,createdAt,body,comments',
  ], { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 })
  return JSON.parse(raw).map(issue => ({
    number: issue.number,
    title: issue.title,
    state: issue.state,
    stateReason: issue.stateReason,
    createdAt: issue.createdAt,
    labels: issue.labels.map(label => label.name),
    comments: issue.comments.length,
    excerpt: issue.body.replace(/\s+/g, ' ').slice(0, 400),
  }))
}

function linkIssues (issueList, commitList) {
  const commitsByIssue = new Map()
  for (const commit of commitList) {
    for (const ref of commit.issueRefs) {
      if (!commitsByIssue.has(ref)) commitsByIssue.set(ref, [])
      commitsByIssue.get(ref).push(commit)
    }
  }
  for (const issue of issueList) {
    const linked = commitsByIssue.get(String(issue.number)) ?? []
    issue.commits = linked.map(pickCommitFields)
    issue.modules = [...new Set(linked.flatMap(commit => commit.sourceFiles.map(moduleOf)))]
  }
}

function summarizeFiles (commitList) {
  const stats = new Map()
  for (const commit of commitList) {
    for (const file of commit.sourceFiles) {
      if (!stats.has(file)) stats.set(file, { file, module: moduleOf(file), lines: lineCounts.get(file) ?? 0, commits: 0, fixes: 0, recentFixes: 0, fixesWithoutTests: 0, fixOnRecentChange: 0 })
      const entry = stats.get(file)
      entry.commits++
      if (commit.kind !== 'fix') continue
      entry.fixes++
      if (now - commit.time <= recentDays * DAY_MS) entry.recentFixes++
      if (commit.testFiles.length === 0) entry.fixesWithoutTests++
    }
  }
  const fixEdgePairs = new Set()
  for (const edge of edges) {
    const key = `${edge.file}:${edge.fix.sha}`
    if (fixEdgePairs.has(key) || !stats.has(edge.file)) continue
    fixEdgePairs.add(key)
    stats.get(edge.file).fixOnRecentChange++
  }
  const rows = [...stats.values()]
  const baseline = median(rows.filter(row => row.fixes > 0).map(row => row.fixes)) || 1
  for (const row of rows) row.heat = Math.round((row.fixes / baseline) * 10) / 10
  return rows.sort((left, right) => right.recentFixes + 2 * right.fixOnRecentChange - left.recentFixes - 2 * left.fixOnRecentChange || right.fixes - left.fixes)
}

function summarizeModules (commitList, edgeList, issueList) {
  const stats = new Map()
  const entryFor = (module) => {
    if (!stats.has(module)) {
      stats.set(module, {
        module, commits: 0, fixes: 0, recentFixes: 0, fixesWithoutTests: 0, reverts: 0,
        fixOnRecentChange: 0, fixOnFix: 0, issues: new Set(), regressionIssues: new Set(),
      })
    }
    return stats.get(module)
  }
  for (const commit of commitList) {
    for (const module of new Set(commit.sourceFiles.map(moduleOf))) {
      const entry = entryFor(module)
      entry.commits++
      if (commit.kind === 'revert') entry.reverts++
      if (commit.kind !== 'fix') continue
      entry.fixes++
      if (now - commit.time <= recentDays * DAY_MS) entry.recentFixes++
      if (commit.testFiles.length === 0) entry.fixesWithoutTests++
    }
  }
  const fixEdgePairs = new Set()
  const fixOnFixPairs = new Set()
  for (const edge of edgeList) {
    const key = `${edge.module}:${edge.fix.sha}`
    if (!fixEdgePairs.has(key)) {
      fixEdgePairs.add(key)
      entryFor(edge.module).fixOnRecentChange++
    }
    if (edge.introducer.kind === 'fix' && !fixOnFixPairs.has(key)) {
      fixOnFixPairs.add(key)
      entryFor(edge.module).fixOnFix++
    }
  }
  for (const [file, lines] of lineCounts) {
    const entry = stats.get(moduleOf(file))
    if (entry) entry.kloc = (entry.kloc ?? 0) + lines / 1000
  }
  for (const issue of issueList) {
    for (const module of issue.modules) {
      entryFor(module).issues.add(issue.number)
      if (issue.labels.includes('regression')) entryFor(module).regressionIssues.add(issue.number)
    }
  }
  const rows = [...stats.values()].map(entry => ({
    ...entry,
    fixShare: entry.commits ? Math.round((entry.fixes / entry.commits) * 100) / 100 : 0,
    issues: [...entry.issues],
    regressionIssues: [...entry.regressionIssues],
  }))
  const baseline = median(rows.filter(row => row.fixes > 0).map(row => row.fixes)) || 1
  for (const row of rows) {
    row.heat = Math.round((row.fixes / baseline) * 10) / 10
    row.fixesPerKloc = row.kloc ? Math.round((row.fixes / row.kloc) * 10) / 10 : null
    row.kloc = Math.round((row.kloc ?? 0) * 10) / 10
    row.score = row.recentFixes + 2 * row.fixOnRecentChange + 3 * row.fixOnFix + 2 * row.regressionIssues.length + row.issues.length + 3 * row.reverts
  }
  return rows.sort((left, right) => right.score - left.score)
}

function summarizeIntroducers (edgeList) {
  const stats = new Map()
  for (const edge of edgeList) {
    const sha = edge.introducer.sha
    if (!stats.has(sha)) stats.set(sha, { introducer: edge.introducer, fixes: new Map(), modules: new Set(), lines: 0 })
    const entry = stats.get(sha)
    entry.fixes.set(edge.fix.sha, edge.fix)
    entry.modules.add(edge.module)
    entry.lines += edge.lines
  }
  return [...stats.values()]
    .map(entry => ({ introducer: entry.introducer, fixes: [...entry.fixes.values()], modules: [...entry.modules], lines: entry.lines }))
    .sort((left, right) => right.fixes.length - left.fixes.length || right.lines - left.lines)
}

function pickCommitFields (commit) {
  return { sha: commit.sha.slice(0, 10), date: new Date(commit.time).toISOString().slice(0, 10), kind: commit.kind, subject: commit.subject }
}

function median (values) {
  if (values.length === 0) return 0
  const sorted = [...values].sort((left, right) => left - right)
  return sorted[Math.floor(sorted.length / 2)]
}

async function mapWithConcurrency (items, limit, worker) {
  const results = new Array(items.length)
  let next = 0
  const runners = Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (next < items.length) {
      const index = next++
      results[index] = await worker(items[index])
    }
  })
  await Promise.all(runners)
  return results
}

function printSummary (data, jsonFile) {
  const lines = []
  const { totals, windows } = data
  lines.push(`# Defect hotspots (${data.ref}, ${data.generatedAt.slice(0, 10)})`, '')
  lines.push(`${totals.commits} commits in ${windows.historyDays} days, ${totals.fixes} fixes, ${totals.reverts} reverts. Blame looks through ${totals.mechanicalCommits} mechanical commits. ` +
    `${totals.recentFixes} fixes in the last ${windows.recentDays} days were blamed. ${totals.issues} issues opened in the last ${windows.issueDays} days.`, '')
  lines.push('## Modules', '')
  lines.push('| module | score | fixes | recent | heat | fixes/kloc | fix share | fixes w/o tests | fix-on-recent | fix-on-fix | reverts | issues (regression) |')
  lines.push('|---|---|---|---|---|---|---|---|---|---|---|---|')
  for (const row of data.modules.slice(0, topCount)) {
    lines.push(`| ${row.module} | ${row.score} | ${row.fixes} | ${row.recentFixes} | ${row.heat}x | ${row.fixesPerKloc} | ${row.fixShare} | ${row.fixesWithoutTests} | ${row.fixOnRecentChange} | ${row.fixOnFix} | ${row.reverts} | ${row.issues.length} (${row.regressionIssues.length}) |`)
  }
  lines.push('', '## Files', '')
  lines.push('| file | lines | fixes | recent | fix-on-recent | heat | commits | fixes w/o tests |')
  lines.push('|---|---|---|---|---|---|---|---|')
  for (const row of data.files.slice(0, topCount)) {
    lines.push(`| ${row.file} | ${row.lines} | ${row.fixes} | ${row.recentFixes} | ${row.fixOnRecentChange} | ${row.heat}x | ${row.commits} | ${row.fixesWithoutTests} |`)
  }
  lines.push('', '## Commits that needed follow-up fixes', '')
  for (const entry of data.introducers.filter(item => item.fixes.length > 1 || item.introducer.kind === 'fix').slice(0, topCount)) {
    lines.push(`- ${entry.introducer.sha} ${entry.introducer.date} ${entry.introducer.subject} (${entry.lines} lines, ${entry.modules.join(', ')})`)
    for (const fix of entry.fixes) lines.push(`  - fixed by ${fix.sha} ${fix.date} ${fix.subject}`)
  }
  if (data.reverts.length > 0) {
    lines.push('', '## Reverts', '')
    for (const revert of data.reverts) lines.push(`- ${revert.sha} ${revert.date} ${revert.subject}`)
  }
  if (data.issues.length > 0) {
    lines.push('', '## Issues by area label', '')
    const byArea = new Map()
    for (const issue of data.issues) {
      const areas = issue.labels.filter(label => label.startsWith('area:'))
      for (const area of areas.length ? areas : ['(no area)']) {
        if (!byArea.has(area)) byArea.set(area, [])
        byArea.get(area).push(issue)
      }
    }
    for (const [area, list] of [...byArea].sort((left, right) => right[1].length - left[1].length)) {
      lines.push(`### ${area} (${list.length})`, '')
      for (const issue of list) {
        const tags = [issue.state.toLowerCase(), ...issue.labels.filter(label => !label.startsWith('area:'))].join(', ')
        const fixed = issue.modules.length ? ` -> ${issue.modules.join(', ')}` : ''
        lines.push(`- ${issue.number} ${issue.title} [${tags}]${fixed}`)
      }
      lines.push('')
    }
  }
  lines.push(`Full data: ${jsonFile}`)
  process.stdout.write(`${lines.join('\n')}\n`)
}
