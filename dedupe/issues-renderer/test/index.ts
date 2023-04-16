import { renderDedupeCheckIssues } from '@pnpm/dedupe.issues-renderer'
import stripAnsi from 'strip-ansi'

describe('renderDedupeCheckIssues', () => {
  test('prints removed packages and updated resolutions', () => {
    expect(stripAnsi(renderDedupeCheckIssues({
      importerIssuesByImporterId: {
        added: [],
        removed: [],
        updated: {
          '.': {
            '@types/node': { type: 'updated', prev: '14.18.42', next: '18.15.11' },
          },
        },
      },
      packageIssuesByDepPath: {
        added: [],
        removed: ['/@types/node/14.18.42'],
        updated: {
          '@types/tar-stream/2.2.2': {
            '@types/node': { type: 'updated', prev: '14.18.42', next: '18.15.11' },
          },
          '@types/tar/6.1.4': {
            '@types/node': { type: 'updated', prev: '14.18.42', next: '18.15.11' },
          },
        },
      },
    }))).toMatchSnapshot()
  })

  test('prints new importers and packages', () => {
    expect(stripAnsi(renderDedupeCheckIssues({
      importerIssuesByImporterId: {
        added: ['packages/a'],
        removed: [],
        updated: {
          '.': {
            'packages/a': { type: 'added', next: '0.0.0' },
          },
        },
      },
      packageIssuesByDepPath: {
        added: [
          // Suppose packages/a added a new @types/node dependency on 18.x.
          '/@types/node/18.15.11',
        ],
        removed: ['/@types/node/14.18.42'],
        updated: {
          '@types/tar-stream/2.2.2': {
            '@types/node': { type: 'updated', prev: '14.18.42', next: '18.15.11' },
          },
          '@types/tar/6.1.4': {
            '@types/node': { type: 'updated', prev: '14.18.42', next: '18.15.11' },
          },
        },
      },
    }))).toMatchSnapshot()
  })

  test('prints removed importer', () => {
    expect(stripAnsi(renderDedupeCheckIssues({
      importerIssuesByImporterId: {
        added: [],
        removed: ['packages/a'],
        updated: {
          '.': {
            '@types/node': { type: 'updated', prev: '18.15.11', next: '14.18.42' },
          },
        },
      },
      packageIssuesByDepPath: {
        added: ['/@types/node/14.18.42'],
        removed: ['/@types/node/18.15.11'],
        updated: {
          '@types/tar-stream/2.2.2': {
            '@types/node': { type: 'updated', prev: '18.15.11', next: '14.18.42' },
          },
          '@types/tar/6.1.4': {
            '@types/node': { type: 'updated', prev: '18.15.11', next: '14.18.42' },
          },
        },
      },
    }))).toMatchSnapshot()
  })
})
