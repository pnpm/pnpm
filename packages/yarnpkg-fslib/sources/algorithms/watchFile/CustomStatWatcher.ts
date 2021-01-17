import {EventEmitter}                                             from 'events';
import {BigIntStats, Stats}                                       from 'fs';

import {StatWatcher, WatchFileCallback, WatchFileOptions, FakeFS} from '../../FakeFS';
import {Path}                                                     from '../../path';
import * as statUtils                                             from '../../statUtils';

export enum Event {
  Change = `change`,
  Stop = `stop`,
}

export enum Status {
  Ready = `ready`,
  Running = `running`,
  Stopped = `stopped`,
}

export function assertStatus<T extends Status>(current: Status, expected: T): asserts current is T {
  if (current !== expected) {
    throw new Error(`Invalid StatWatcher status: expected '${expected}', got '${current}'`);
  }
}

// `bigint` can only be set class-wide, because that's what Node does
export type ListenerOptions = Omit<Required<WatchFileOptions>, 'bigint'>;

export type CustomStatWatcherOptions = {
  // BigInt Stats aren't currently implemented in the FS layer, so this is a no-op
  bigint?: boolean,
};

export class CustomStatWatcher<P extends Path> extends EventEmitter implements StatWatcher {
  public readonly fakeFs: FakeFS<P>;

  public readonly path: P;

  public readonly bigint: boolean;

  private status: Status = Status.Ready;

  private changeListeners: Map<WatchFileCallback, NodeJS.Timeout> = new Map();

  private lastStats: Stats | BigIntStats;

  private startTimeout: NodeJS.Timeout | null = null;

  static create<P extends Path>(fakeFs: FakeFS<P>, path: P, opts?: CustomStatWatcherOptions) {
    const statWatcher = new CustomStatWatcher<P>(fakeFs, path, opts);

    statWatcher.start();

    return statWatcher;
  }

  private constructor(fakeFs: FakeFS<P>, path: P, {bigint = false}: CustomStatWatcherOptions = {}) {
    super();

    this.fakeFs = fakeFs;
    this.path = path;
    this.bigint = bigint;

    this.lastStats = this.stat();
  }

  start() {
    assertStatus(this.status, Status.Ready);
    this.status = Status.Running;

    // Node allows other listeners to be registered up to 3 milliseconds
    // after the watcher has been started, so that's what we're doing too
    this.startTimeout = setTimeout(() => {
      this.startTimeout = null;

      // Per the Node FS docs:
      // "When an fs.watchFile operation results in an ENOENT error,
      // it will invoke the listener once, with all the fields zeroed
      // (or, for dates, the Unix Epoch)."
      if (!this.fakeFs.existsSync(this.path)) {
        this.emit(Event.Change, this.lastStats, this.lastStats);
      }
    }, 3);
  }

  stop() {
    assertStatus(this.status, Status.Running);
    this.status = Status.Stopped;

    if (this.startTimeout !== null) {
      clearTimeout(this.startTimeout);
      this.startTimeout = null;
    }

    this.emit(Event.Stop);
  }

  stat() {
    try {
      return this.fakeFs.statSync(this.path, {bigint: this.bigint});
    } catch (error) {
      if (error.code === `ENOENT`) {
        const statInstance = this.bigint
          ? ((new statUtils.BigIntStatsEntry() as unknown) as BigIntStats)
          : ((new statUtils.StatEntry() as unknown) as Stats);

        return statUtils.clearStats(statInstance);
      } else {
        throw error;
      }
    }
  }

  /**
   * Creates an interval whose callback compares the current stats with the previous stats and notifies all listeners in case of changes.
   *
   * @param opts.persistent Decides whether the interval should be immediately unref-ed.
   */
  makeInterval(opts: ListenerOptions) {
    const interval = setInterval(() => {
      const currentStats = this.stat();
      const previousStats = this.lastStats;

      if (statUtils.areStatsEqual(currentStats, previousStats))
        return;

      this.lastStats = currentStats;

      this.emit(Event.Change, currentStats, previousStats);
    }, opts.interval);

    return opts.persistent ? interval : interval.unref();
  }

  /**
   * Registers a listener and assigns it an interval.
   */
  registerChangeListener(listener: WatchFileCallback, opts: ListenerOptions) {
    this.addListener(Event.Change, listener);

    this.changeListeners.set(listener, this.makeInterval(opts));
  }

  /**
   * Unregisters the listener and clears the assigned interval.
   */
  unregisterChangeListener(listener: WatchFileCallback) {
    this.removeListener(Event.Change, listener);

    const interval = this.changeListeners.get(listener);
    if (typeof interval !== `undefined`)
      clearInterval(interval);

    this.changeListeners.delete(listener);
  }

  /**
   * Unregisters all listeners and clears all assigned intervals.
   */
  unregisterAllChangeListeners() {
    for (const listener of this.changeListeners.keys()) {
      this.unregisterChangeListener(listener);
    }
  }

  hasChangeListeners() {
    return this.changeListeners.size > 0;
  }

  /**
   * Refs all stored intervals.
   */
  ref() {
    for (const interval of this.changeListeners.values())
      interval.ref();

    return this;
  }

  /**
   * Unrefs all stored intervals.
   */
  unref() {
    for (const interval of this.changeListeners.values())
      interval.unref();

    return this;
  }
}
