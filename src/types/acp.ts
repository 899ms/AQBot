export interface AcpGeneralConfig {
  idleTimeoutSecs: number;
  maxConcurrentProcesses: number;
  permissionDefault: string;
  registryRefresh: string;
}

export interface ConfiguredAgent {
  id: string;
  name: string;
  enabled: boolean;
  source: string;
  command: string;
  args: string[];
  env?: Record<string, string>;
  icon?: string | null;
  sort: number;
}

export interface AcpAgentsFile {
  general: AcpGeneralConfig;
  agents: ConfiguredAgent[];
}

export interface RegistryAgent {
  id: string;
  name: string;
  version?: string | null;
  description?: string | null;
  repository?: string | null;
  website?: string | null;
  icon?: string | null;
  license?: string | null;
}

export interface RegistryFile {
  version: string;
  agents: RegistryAgent[];
  source?: 'builtin' | 'cache' | 'live' | null;
  fetchedAt?: string | null;
}

export interface AcpProject {
  id: string;
  name: string;
  root_path: string;
  sort_order: number;
  created_at: string;
  updated_at: string;
  last_opened_at?: string | null;
}

export interface AcpThread {
  id: string;
  project_id: string;
  agent_id: string;
  title: string;
  acp_session_id?: string | null;
  runtime_status: string;
  mode_id?: string | null;
  created_at: string;
  updated_at: string;
}

export interface AcpMessage {
  id: string;
  thread_id: string;
  role: string;
  content: string;
  status?: string | null;
  attachments_json?: string | null;
  meta_json?: string | null;
  created_at: string;
}

export interface AgentProbeResult {
  agentId: string;
  available: boolean;
  command: string;
  message: string;
}
