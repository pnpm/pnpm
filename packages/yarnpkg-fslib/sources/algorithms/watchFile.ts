import {FakeFS, WatchFileOptions, WatchFileCallback} from '../FakeFS';
import {Path}                                        from '../path';

import {CustomStatWatcher}                           from './watchFile/CustomStatWatcher';

const statWatchersByFakeFS = new WeakMap<FakeFS<Path>, Map<Path, CustomStatWatcher<Path>>>();

export function watchFile<P extends Path>(
  fakeFs: FakeFS<P>,
  path: P,
  a: WatchFileOptions | WatchFileCallback,
  b?: WatchFileCallback
) {
  let bigint: boolean;
  let persistent: boolean;
  let interval: number;

  let listener: WatchFileCallback;

  switch (typeof a) {
    case `function`: {
      bigint = false;
      persistent = true;
      interval = 5007;

      listener = a;
    } break;

    default: {
      ({
        bigint = false,
        persistent = true,
        interval = 5007,
      } = a);

      listener = b!;
    } break;
  }

  let statWatchers = statWatchersByFakeFS.get(fakeFs);
  if (typeof statWatchers === `undefined`)
    statWatchersByFakeFS.set(fakeFs, statWatchers = new Map());

  let statWatcher = statWatchers.get(path);
  if (typeof statWatcher === `undefined`) {
    statWatcher = CustomStatWatcher.create<P>(fakeFs, path, {bigint});

    statWatchers.set(path, statWatcher);
  }

  statWatcher.registerChangeListener(listener, {persistent, interval});

  return statWatcher as CustomStatWatcher<P>;
}

export function unwatchFile<P extends Path>(fakeFs: FakeFS<P>, path: P, cb?: WatchFileCallback) {
  const statWatchers = statWatchersByFakeFS.get(fakeFs);
  if (typeof statWatchers === `undefined`)
    return;

  const statWatcher = statWatchers.get(path);
  if (typeof statWatcher === `undefined`)
    return;

  if (typeof cb === `undefined`)
    statWatcher.unregisterAllChangeListeners();
  else
    statWatcher.unregisterChangeListener(cb);

  if (!statWatcher.hasChangeListeners()) {
    statWatcher.stop();
    statWatchers.delete(path);
  }
}

export function unwatchAllFiles(fakeFs: FakeFS<Path>) {
  const statWatchers = statWatchersByFakeFS.get(fakeFs);
  if (typeof statWatchers === `undefined`)
    return;

  for (const path of statWatchers.keys()) {
    unwatchFile(fakeFs, path);
  }
}
