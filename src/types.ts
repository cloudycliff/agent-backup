export type GroupView = { id: string; label: string; default_enabled: boolean };

export type AgentView = {
  key: string;
  label: string;
  installed: boolean;
  disabled: boolean;
  root_path?: string | null;
  groups: GroupView[];
  enabled: boolean;
  paths: Record<string, boolean>;
};

export type Destination =
  | {
      type: "local";
      id: string;
      name: string;
      enabled: boolean;
      local: { root_path: string };
    }
  | {
      type: "s3";
      id: string;
      name: string;
      enabled: boolean;
      s3: Record<string, string>;
    };

export type CustomSource = {
  id: string;
  label: string;
  path: string;
  enabled: boolean;
};

export type AppConfig = {
  destinations: Destination[];
  sources: {
    agents: Record<string, unknown>;
    custom: CustomSource[];
  };
  encryption: { enabled: boolean; password: string | null };
  schedule: { enabled: boolean; time_local: string };
  exclusions: { file_name_globs: string[]; dir_name_globs: string[] };
  app: {
    hostname_override: string | null;
    start_on_login: boolean;
    minimize_to_tray: boolean;
    language: string;
  };
};

export type Bootstrap = {
  config: AppConfig;
  agents: AgentView[];
  config_dir: string;
  config_path: string;
  presets_path: string;
  hostname: string;
  warnings: string[];
};

export type BackupResult = {
  id?: string;
  created_at: string;
  overall_status: string;
  message: string;
  destinations: Array<{
    name: string;
    status: string;
    uri?: string | null;
    error?: string | null;
  }>;
  sources?: Array<{
    key: string;
    label: string;
    status: string;
    archive?: string;
    bytes?: number;
    error?: string | null;
    warnings?: string[];
  }>;
};

export type HistoryEntry = {
  id: string;
  created_at: string;
  hostname: string;
  trigger: string;
  overall_status: string;
  message: string;
  sources: BackupResult["sources"];
  destinations: BackupResult["destinations"];
};

export type StatusView = {
  last_success: HistoryEntry | null;
  next_run: { at: string; overdue: boolean } | null;
};

export type Estimate = { file_count: number; bytes: number };

export type Progress = {
  phase: string;
  current: number;
  total: number;
  label: string;
  bytes_done: number;
  bytes_total: number;
  percent: number;
};

export type Tab = "backup" | "destinations" | "history" | "settings";

/** Runs a config-changing command and applies the returned bootstrap state. */
export type WithRefresh = (fn: () => Promise<Bootstrap>) => Promise<void>;
