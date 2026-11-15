export type PluginCategory = 'application' | 'account' | 'appearance' | 'feature' | 'other';

export interface PluginProviderContribution {
  id: string;
  title: string;
  applications: string[];
  pageId?: string;
  searchTerms?: string[];
  operations?: string[];
}

export interface PluginManifest {
  name: string;
  publisher: string;
  version: string;
  displayName: string;
  description: string;
  license: string;
  repository: string;
  icon?: string;
  architectures?: ('x64' | 'arm64')[];
  category?: PluginCategory;
  categories?: PluginCategory[];
  tools?: { id: string; directory: string }[];
  echobird: {
    manifestVersion: number;
    apiVersion: string;
    platforms: string[];
    permissions: string[];
    contributes: {
      pages: { id: string; title: string; entry: string }[];
      providers?: PluginProviderContribution[];
      themes?: PluginTheme[];
      configuration?: PluginConfiguration[];
      commands?: { id: string; title: string; pageId?: string }[];
      textProcessors?: { id: string; title: string; stage: 'voice.transcribed' | 'voice.output' }[];
    };
    background?: string;
    worker?: { entry: string; runtime?: 'node' | 'native' };
  };
}

export interface InstalledPlugin {
  id: string;
  manifest: PluginManifest;
  enabled: boolean;
  directory: string;
  source: string;
  revision: string | null;
  icon: string | null;
}

export interface PluginTheme {
  id: string;
  title: string;
  light: Record<string, string>;
  dark: Record<string, string>;
}
export interface PluginConfiguration {
  key: string;
  title: string;
  type: 'boolean' | 'number' | 'string';
  default: boolean | number | string;
  options?: (boolean | number | string)[];
}
export interface PluginSelection {
  pluginId: string;
  id: string;
}
export interface PluginProductState {
  theme: PluginSelection | null;
  palette: PluginTheme | null;
  processors: PluginSelection[];
  configuration: Record<string, Record<string, boolean | number | string>>;
  errors: Record<string, string>;
}
export interface PluginRegistry {
  schemaVersion: number;
  name: string;
  plugins: {
    id: string;
    repository: string;
    clientIcon?: string;
    descriptions?: Record<PluginIndexRegion, string>;
    icons?: Partial<Record<PluginIndexRegion, string>>;
    releases: {
      manifest: PluginManifest;
      commit: string;
      sha256: string;
      downloads: Record<string, string>;
    }[];
  }[];
}

export interface PluginCandidate {
  token: string;
  manifest: PluginManifest;
  source: string;
  revision: string | null;
}

export type PluginRegion = 'international' | 'china' | 'cnb';
/** CNB mirrors GitCode, so it reads the China entries of the index. */
export type PluginIndexRegion = Exclude<PluginRegion, 'cnb'>;
export type PluginSource = 'directory' | 'repository' | 'catalog';
