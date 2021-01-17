import * as statUtils from './statUtils';

export {opendir} from './algorithms/opendir';

export {statUtils};

export {normalizeLineEndings}          from './FakeFS';
export type {CreateReadStreamOptions}  from './FakeFS';
export type {CreateWriteStreamOptions} from './FakeFS';
export type {Dirent, Dir, SymlinkType} from './FakeFS';
export type {MkdirOptions}             from './FakeFS';
export type {RmdirOptions}             from './FakeFS';
export type {WatchOptions}             from './FakeFS';
export type {WatchCallback}            from './FakeFS';
export type {Watcher}                  from './FakeFS';
export type {WriteFileOptions}         from './FakeFS';
export type {ExtractHintOptions}       from './FakeFS';
export type {WatchFileOptions}         from './FakeFS';
export type {WatchFileCallback}        from './FakeFS';
export type {StatWatcher}              from './FakeFS';
export type {OpendirOptions}           from './FakeFS';

export {DEFAULT_COMPRESSION_LEVEL}     from './ZipFS';
export type {ZipCompression}           from './ZipFS';

export {PortablePath, Filename}                            from './path';
export type {FSPath, Path, NativePath}                     from './path';
export type {ParsedPath, PathUtils, FormatInputPathObject} from './path';
export {npath, ppath, toFilename}                          from './path';

export {AliasFS}                   from './AliasFS';
export {FakeFS}                    from './FakeFS';
export {CwdFS}                     from './CwdFS';
export {JailFS}                    from './JailFS';
export {LazyFS}                    from './LazyFS';
export {NoFS}                      from './NoFS';
export {NodeFS}                    from './NodeFS';
export {PosixFS}                   from './PosixFS';
export {ProxiedFS}                 from './ProxiedFS';
export {VirtualFS}                 from './VirtualFS';
export {ZipFS}                     from './ZipFS';
export {ZipOpenFS}                 from './ZipOpenFS';

export {patchFs, extendFs} from './patchFs';

export {xfs} from './xfs';
export type {XFS} from './xfs';
