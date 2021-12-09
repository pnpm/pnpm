import renderPeerIssues from '@pnpm/render-peer-issues'

test('renderPeerIssues()', () => {
  expect(renderPeerIssues({
    missing: {},
    bad: {},
  })).toEqual('')
})
