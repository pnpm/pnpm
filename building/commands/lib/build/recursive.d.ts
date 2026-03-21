import { type Config } from '@pnpm/config.reader';
import { type CreateStoreControllerOptions } from '@pnpm/store.connection-manager';
import type { Project } from '@pnpm/types';
type RecursiveRebuildOpts = CreateStoreControllerOptions & Pick<Config, 'hoistPattern' | 'hooks' | 'ignorePnpmfile' | 'ignoreScripts' | 'lockfileDir' | 'lockfileOnly' | 'nodeLinker' | 'packageConfigs' | 'rawLocalConfig' | 'registries' | 'rootProjectManifest' | 'rootProjectManifestDir' | 'sharedWorkspaceLockfile'> & {
    pending?: boolean;
} & Partial<Pick<Config, 'bail' | 'sort' | 'workspaceConcurrency'>>;
export declare function recursiveRebuild(allProjects: Project[], params: string[], opts: RecursiveRebuildOpts & {
    ignoredPackages?: Set<string>;
} & Required<Pick<Config, 'selectedProjectsGraph' | 'workspaceDir'>>): Promise<void>;
export {};
