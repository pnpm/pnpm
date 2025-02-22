import { type DirDiff, type InodeMap, DIR, diffDir } from '../src/DirPatcher'

test('produces a diff', () => {
  const unchangedParts = {
    'not-changed': DIR,
    'not-changed/foo': DIR,
    'not-changed/foo/foo.txt': 123,
    'not-changed/foo/bar.txt': 456,
    'not-changed/bar': DIR,
    'some-files-changed/not-changed.txt': 623,
    'some-parts-deleted/file-not-deleted.txt': 624,
    'some-parts-added/file-not-added.txt': 145,
  } satisfies InodeMap

  const oldModifiedParts = {
    'some-files-changed': DIR,
    'some-files-changed/changed-file.txt': 887,
  } satisfies InodeMap

  const newModifiedParts: typeof oldModifiedParts = {
    'some-files-changed': DIR,
    'some-files-changed/changed-file.txt': 553,
  }

  const oldOnlyParts = {
    'some-parts-deleted': DIR,
    'some-parts-deleted/file-deleted.txt': 654,
    'some-parts-deleted/dir-deleted': DIR,
    'some-parts-deleted/dir-deleted/foo.txt': 325,
    'some-parts-deleted/dir-deleted/bar.txt': 231,
  } satisfies InodeMap

  const newOnlyParts = {
    'some-parts-added': DIR,
    'some-parts-added/file-added.txt': 362,
    'some-parts-added/dir-added': DIR,
    'some-parts-added/dir-added/foo.txt': 472,
    'some-parts-added/dir-added/bar.txt': 241,
  } satisfies InodeMap

  const oldIndex: InodeMap = {
    ...unchangedParts,
    ...oldModifiedParts,
    ...oldOnlyParts,
  }

  const newIndex: InodeMap = {
    ...unchangedParts,
    ...newModifiedParts,
    ...newOnlyParts,
  }

  const expectedDiff: DirDiff = {
    added: [
      {
        path: 'some-parts-added',
        newValue: DIR,
      },
      {
        path: 'some-parts-added/dir-added',
        newValue: DIR,
      },
      {
        path: 'some-parts-added/file-added.txt',
        newValue: newOnlyParts['some-parts-added/file-added.txt'],
      },
      {
        path: 'some-parts-added/dir-added/bar.txt',
        newValue: newOnlyParts['some-parts-added/dir-added/bar.txt'],
      },
      {
        path: 'some-parts-added/dir-added/foo.txt',
        newValue: newOnlyParts['some-parts-added/dir-added/foo.txt'],
      },
    ],
    modified: [
      {
        path: 'some-files-changed/changed-file.txt',
        oldValue: oldModifiedParts['some-files-changed/changed-file.txt'],
        newValue: newModifiedParts['some-files-changed/changed-file.txt'],
      },
    ],
    removed: [
      {
        path: 'some-parts-deleted',
        oldValue: DIR,
      },
      {
        path: 'some-parts-deleted/dir-deleted',
        oldValue: DIR,
      },
      {
        path: 'some-parts-deleted/file-deleted.txt',
        oldValue: oldOnlyParts['some-parts-deleted/file-deleted.txt'],
      },
      {
        path: 'some-parts-deleted/dir-deleted/bar.txt',
        oldValue: oldOnlyParts['some-parts-deleted/dir-deleted/bar.txt'],
      },
      {
        path: 'some-parts-deleted/dir-deleted/foo.txt',
        oldValue: oldOnlyParts['some-parts-deleted/dir-deleted/foo.txt'],
      },
    ],
  }

  const receivedDiff = diffDir(oldIndex, newIndex)

  expect(receivedDiff).toStrictEqual(expectedDiff)
})
