import {NativePath, PortablePath, Path} from '@yarnpkg/fslib';

// Note: most of those types are useless for most users. Just check the
// PnpSettings and PnpApi types at the end and you'll be fine.
//
// Apart from that, note that the "Data"-suffixed types are the ones stored
// within the state files (hence why they only use JSON datatypes).

export enum LinkType {
  HARD = `HARD`, SOFT = `SOFT`,
}

export type PhysicalPackageLocator = {name: string, reference: string};
export type TopLevelPackageLocator = {name: null, reference: null};

export type PackageLocator = PhysicalPackageLocator | TopLevelPackageLocator;

export type DependencyTarget =
  // A reference, to link with the dependency name
  | string
  // An aliased package
  | [string, string]
  // A missing peer dependency
  | null;

export type PackageInformation<P extends Path> = {packageLocation: P, packageDependencies: Map<string, DependencyTarget>, packagePeers: Set<string>, linkType: LinkType, discardFromLookup: boolean};
export type PackageInformationData<P extends Path> = {packageLocation: P, packageDependencies: Array<[string, DependencyTarget]>, packagePeers?: Array<string>, linkType: LinkType, discardFromLookup?: boolean};

export type PackageStore = Map<string | null, PackageInformation<PortablePath>>;
export type PackageStoreData = Array<[string | null, PackageInformationData<PortablePath>]>;

export type PackageRegistry = Map<string | null, PackageStore>;
export type PackageRegistryData = Array<[string | null, PackageStoreData]>;

export type LocationBlacklistData = Array<PortablePath>;
export type LocationLengthData = Array<number>;

// This is what is stored within the .pnp.meta.json file
export type SerializedState = {
  // @eslint-ignore-next-line @typescript-eslint/naming-convention
  __info: Array<string>;
  enableTopLevelFallback: boolean,
  fallbackExclusionList: Array<[string, Array<string>]>,
  fallbackPool: Array<[string, DependencyTarget]>,
  ignorePatternData: string | null,
  locationBlacklistData: LocationBlacklistData,
  packageRegistryData: PackageRegistryData,
  dependencyTreeRoots: Array<PhysicalPackageLocator>,
};

// This is what `makeApi` actually consumes
export type RuntimeState = {
  basePath: PortablePath,
  enableTopLevelFallback: boolean,
  fallbackExclusionList: Map<string, Set<string>>,
  fallbackPool: Map<string, DependencyTarget>,
  ignorePattern: RegExp | null,
  packageLocationLengths: Array<number>,
  packageLocatorsByLocations: Map<PortablePath, PhysicalPackageLocator | null>;
  packageRegistry: PackageRegistry,
  dependencyTreeRoots: Array<PhysicalPackageLocator>,
};

// This is what the generation functions take as parameter
export type PnpSettings = {
  // Some locations that are not allowed to make a require call, period
  // (usually the realpath of virtual packages)
  blacklistedLocations?: Iterable<PortablePath>,

  // Whether the top-level dependencies should be made available to all the
  // dependency tree as a fallback (default is true)
  enableTopLevelFallback?: boolean,

  // Which packages should never be allowed to use fallbacks, no matter what
  fallbackExclusionList?: Array<PhysicalPackageLocator>,

  // Which packages should be made available through the fallback mechanism
  fallbackPool?: Map<string, DependencyTarget>,

  // Which paths shouldn't use PnP, even if they would otherwise be detected
  // as being owned by a package (legacy settings used to help people migrate
  // to PnP + workspaces when they weren't using either)
  ignorePattern?: string | null,

  // The set of packages to store within the PnP map
  packageRegistry: PackageRegistry,

  // The shebang to add at the top of the file, can be any string you want (the
  // default value should be enough most of the time)
  shebang?: string | null,

  // The following locators will be made available in the API through the
  // getDependencyTreeRoots function. They are typically the workspace
  // locators.
  dependencyTreeRoots: Array<PhysicalPackageLocator>,
};

export type PnpApi = {
  VERSIONS: {std: number, [key: string]: number},

  topLevel: {name: null, reference: null},
  getLocator: (name: string, referencish: string | [string, string]) => PhysicalPackageLocator,

  getDependencyTreeRoots: () => Array<PhysicalPackageLocator>,
  getPackageInformation: (locator: PackageLocator) => PackageInformation<NativePath> | null,
  findPackageLocator: (location: NativePath) => PhysicalPackageLocator | null,

  resolveToUnqualified: (request: string, issuer: NativePath | null, opts?: {considerBuiltins?: boolean}) => NativePath | null,
  resolveUnqualified: (unqualified: NativePath, opts?: {extensions?: Array<string>}) => NativePath,
  resolveRequest: (request: string, issuer: NativePath | null, opts?: {considerBuiltins?: boolean, extensions?: Array<string>}) => NativePath | null,

  // Extension methods
  resolveVirtual?: (p: NativePath) => NativePath | null,
  getAllLocators?: () => Array<PhysicalPackageLocator>,
};
