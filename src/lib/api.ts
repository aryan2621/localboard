import { invoke } from '@tauri-apps/api/core'

function call<T>(command: string, args?: Record<string, unknown>) {
  if (!(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) {
    return Promise.reject(new Error('LocalBoard only runs in the desktop app, not in a browser.'))
  }
  return invoke<T>(command, args)
}

export type Hardware = {
  ramGb: number
  cpu: string
  cores: number
  os: string
  arch: string
  accelerator: string
}

export type LocalServerInfo = { port: number; url: string }

export type AppStatus = {
  version: string
  onboardingCompleted: boolean
  hardware: Hardware
  localServer: LocalServerInfo | null
  localServerError: string | null
  offline: boolean
  locked: boolean
  lockEnabled: boolean
  ready: boolean
  needsPassphrase: string | null
}

export type Workspace = { id: string; name: string; created_at: string; last_opened_at: string | null; encrypted: boolean }


export type GovernanceStatus = {
  locked: boolean
  lockEnabled: boolean
  offline: boolean
  encrypted: boolean
  encryptionScope: string
}

export type LocalMcpInfo = { url: string; token: string; proxyCommand: string }
export type ColumnMask = { id: string; relation: string; columnName: string; strategy: 'redact' | 'hash' | 'email' }
export type McpServer = { id: string; name: string; transport: string; command: string | null; url: string | null; enabled: boolean }
export type McpTool = { name: string; description: string; readOnly: boolean }
export type McpToolRule = { agentId: string; serverId: string; toolName: string; allow: boolean }
export type WorkspaceList = { workspaces: Workspace[]; activeId: string | null }

export type DatabaseKind =
  | 'postgres'
  | 'mysql'
  | 'sqlite'
  | 'sqlserver'
  | 'supabase'
  | 'redshift'
  | 'motherduck'
  | 'clickhouse'
  | 'snowflake'
  | 'bigquery'
  | 'databricks'
export type ConnectorKind = 'file' | 'folder' | DatabaseKind
export type AccessMode = 'live' | 'snapshot'

export type Connection = {
  id: string
  kind: ConnectorKind
  name: string
  detail: string
  tableCount: number
  sourceMissing: boolean
  createdAt: string
  mode: AccessMode
  /** Oldest snapshot time across this connection's tables; null for live connections. */
  snapshotAt: string | null
  snapshotTruncated: boolean
  rowCap: number | null
}

export type SyncReport = { tables: number; errors: string[] }
export type AddSourceResult = { connection: Connection; report: SyncReport }

export type DbConfig = {
  name: string
  kind: DatabaseKind
  host: string
  port: number
  database: string
  username: string
  /** The connection's one secret (password, token or key). Stored in the keychain only. */
  password: string
  extra: Record<string, string>
}

export type ExploredTable = {
  name: string
  row_count: number
  columns: { name: string; data_type: string }[]
  samples: SampleRow[]
}

export type SampleRow = Record<string, unknown>

export type TableRecord = {
  id: string
  connection_id: string
  source_ref: string
  relation: string
  description: string
  row_count: number | null
  profile_error: string | null
  profiled_at: string | null
}

export type ColumnProfile = {
  name: string
  data_type: string
  null_pct: number
  distinct_count: number
  min_value: string | null
  max_value: string | null
  sample_values: string[]
  top_values: { value: string; count: number }[]
}

export type ColumnRecord = {
  name: string
  ordinal: number
  data_type: string
  description: string
  profile: ColumnProfile | null
}

export type Relationship = {
  direction: 'outgoing' | 'incoming'
  column: string
  otherTableId: string
  otherRelation: string
  otherColumn: string
  confidence: number
  origin: 'detected' | 'user'
}

export type UsageRef = { id: string; name: string }
export type TileUsage = { id: string; name: string; dashboardId: string; dashboardName: string }
export type ArtifactUsage = { id: string; name: string; parentId: string; parentName: string }
export type TableUsage = {
  lastAuditAt: string | null
  chats: UsageRef[]
  savedQueries: UsageRef[]
  tiles: TileUsage[]
  automations: UsageRef[]
  datasites: ArtifactUsage[]
  reports: ArtifactUsage[]
  decks: ArtifactUsage[]
}
export type Dependent = { kind: string; id: string; name: string }

export type TableDetail = {
  table: TableRecord
  columns: ColumnRecord[]
  samples: SampleRow[]
  relationships: Relationship[]
  usedBy: TableUsage
}

export type DownloadProgress = { id: string; done: number; total: number; error: string | null; finished: boolean }

export type TaskRouting = { sql: string | null; reasoning: string | null; narrative: string | null }

export type LocalModel = {
  id: string
  name: string
  sizeMb: number
  minRamGb: number
  note: string
  installed: boolean
}

export type ModelCatalog = {
  ramGb: number
  recommended: string
  runtimeAvailable: boolean
  running: string | null
  download: DownloadProgress | null
  routing: TaskRouting
  active: string | null
  models: LocalModel[]
  embedding: LocalModel
}

export type Benchmark = { modelId: string; loadMs: number; tokensPerSec: number; completionTokens: number }

export type ColumnKind = 'number' | 'temporal' | 'text' | 'boolean' | 'other'
export type ResultColumn = { name: string; dataType: string; kind: ColumnKind }
export type QueryResult = { columns: ResultColumn[]; rows: unknown[][]; truncated: boolean }

export type QueryPurpose = 'answer' | 'comparison' | 'breakdown'
export type QueryStatus = 'ok' | 'error' | 'denied'

export type QueryRecord = {
  id: string
  messageId: string
  purpose: QueryPurpose
  attempt: number
  sql: string
  relations: string[]
  status: QueryStatus
  rowCount: number | null
  durationMs: number | null
  error: string | null
  result: QueryResult | null
  createdAt: string
}

export type AnswerMode = 'ai' | 'sql' | 'why' | 'clarification'
export type StepStatus = 'running' | 'done' | 'error' | 'skipped'
export type Step = { id: string; label: string; status: StepStatus; detail: string | null }
export type ChartKind = 'kpi' | 'line' | 'bar' | 'stacked' | 'area' | 'pie' | 'table' | 'funnel' | 'heatmap' | 'text'
export type ChartSpec = { kind: ChartKind; queryId: string; x: string | null; y: string[]; dashed?: string[] }
export type AnswerPart = { question: string; chart: ChartSpec }
export type Anomaly = { period: unknown; measure: string; value: number; zScore: number; rule: 'zscore' | 'iqr' | 'both' }
export type CompletionSources = { questions: string[]; terms: string[] }

export type Driver = {
  dimension: string
  segment: unknown
  previous: number
  current: number
  change: number
  shareOfChange: number | null
}

export type Answer = {
  mode: AnswerMode
  steps: Step[]
  explanation: string | null
  assumptions: string[]
  warnings: string[]
  clarification: { question: string; options: string[] } | null
  chart: ChartSpec | null
  primaryQueryId: string | null
  drivers: Driver[]
  model: string | null
  /** Absent on answers saved before forecasting and multi-part answers existed. */
  anomalies?: Anomaly[]
  parts?: AnswerPart[]
  /** Absent on answers saved before analysts existed. */
  agentId?: string | null
  agentName?: string | null
  memoryProposal?: { id: string; body: string } | null
  /** Absent on answers saved before lineage was recorded. */
  lineage?: {
    modelId: string | null
    rules: { id: string; title: string; version: string }[]
    examples: string[]
    memories: string[]
    knowledge: { doc: string; chunk: number }[]
    columns: string[]
  } | null
}

export type Agent = {
  id: string
  name: string
  description: string
  modelOverrides: TaskRouting
  assignedTables: string[]
  createdAt: string
}

export type Rule = {
  id: string
  agentId: string
  title: string
  body: string
  tableName: string | null
  updatedAt: string
  pending: RuleSnapshot | null
  promoted: boolean
}

export type RuleSnapshot = { title: string; body: string; tableName: string | null }
export type RuleVersion = {
  id: string
  snapshot: RuleSnapshot
  evalScore: number | null
  promotedAt: string | null
  createdAt: string
}

export type SchemaTable = { relation: string; columns: string[] }
export type SavedQuery = { id: string; name: string; sql: string; updatedAt: string }
export type HistoryEntry = {
  id: string
  sql: string
  paramsJson: string
  status: string
  error: string | null
  rowCount: number | null
  durationMs: number | null
  createdAt: string
}
export type BenchOutcome = { sql: string; result: QueryResult; durationMs: number }
export type Example = {
  id: string
  agentId: string
  question: string
  sql: string
  origin: 'manual' | 'thumbs_up'
  messageId: string | null
  createdAt: string
}

export type MemoryStatus = 'proposed' | 'active' | 'archived'
export type Memory = {
  id: string
  agentId: string
  body: string
  status: MemoryStatus
  sourceMessageId: string | null
  updatedAt: string
}

export type KnowledgeFile = { id: string; name: string; chunkCount: number; createdAt: string }

export type FilterDef = { name: string; label: string; kind: string }
export type TileLayout = { x: number; y: number; w: number; h: number }
export type DashboardFolder = { id: string; name: string }
export type DashboardSummary = { id: string; name: string; folderId: string | null; folderName: string | null; tileCount: number; updatedAt: string }
export type DashboardTile = {
  id: string
  dashboardId: string
  title: string
  sql: string
  chart: ChartSpec
  layout: TileLayout
  insight: string | null
  factsHash: string | null
}
export type DashboardRecord = {
  id: string
  name: string
  folderId: string | null
  prompt: string
  filters: FilterDef[]
  refreshSecs: number | null
  tiles: DashboardTile[]
}
export type LiveTile = DashboardTile & { result: QueryResult | null; error: string | null }
export type LiveDashboard = { dashboard: DashboardRecord; tiles: LiveTile[]; warnings: string[] }
export type DashboardVersion = { id: string; createdAt: string }

export type OutputKind = 'answer' | 'dashboard' | 'datasite' | 'report' | 'deck' | 'automation'
export type WorkspaceTheme = { brand: string; name: string }
export type ArtifactSummary = { id: string; name: string; updatedAt: string; itemCount: number }
export type DatasiteChart = {
  id: string
  pageId: string
  title: string
  sql: string
  chart: ChartSpec
  result: QueryResult | null
  facts: unknown
  sort: number
}
export type DatasitePage = {
  id: string
  datasiteId: string
  slug: string
  title: string
  sort: number
  narrative: string
  charts: DatasiteChart[]
}
export type DatasiteRecord = {
  id: string
  name: string
  prompt: string
  sourceMessageId: string | null
  pages: DatasitePage[]
}
export type GeneratedDatasite = { datasite: DatasiteRecord; url: string; warnings: string[] }
export type ReportBlock = {
  id: string
  reportId: string
  kind: string
  title: string
  body: string
  sql: string | null
  chart: ChartSpec | null
  result: QueryResult | null
  facts: unknown
  sort: number
}
export type ReportRecord = {
  id: string
  name: string
  prompt: string
  theme: { brand?: string; name?: string }
  sourceMessageId: string | null
  blocks: ReportBlock[]
}
export type GeneratedReport = { report: ReportRecord; warnings: string[] }
export type OutlineSlide = { id: string; title: string; body: string }
export type DeckSlide = {
  id: string
  deckId: string
  title: string
  body: string
  sql: string | null
  chart: ChartSpec | null
  result: QueryResult | null
  sort: number
}
export type DeckRecord = {
  id: string
  name: string
  prompt: string
  outline: OutlineSlide[]
  sourceMessageId: string | null
  slides: DeckSlide[]
}
export type GeneratedDeck = { deck: DeckRecord; warnings: string[] }

export type TriggerKind = 'cron' | 'interval' | 'file_change' | 'manual' | 'webhook'
export type StepKind = 'sql' | 'ask' | 'dashboard' | 'compare' | 'threshold' | 'anomaly' | 'report' | 'email' | 'mcp_tool' | 'webhook' | 'write_file' | 'notify' | 'branch'
export type AutomationStep = { id: string; sort: number; kind: StepKind; config: Record<string, unknown> }
export type Automation = {
  id: string
  name: string
  triggerKind: TriggerKind
  trigger: Record<string, unknown>
  steps: AutomationStep[]
  enabled: boolean
  onChangeOnly: boolean
  needsReview: boolean
  source: 'user' | 'ai'
  confirmedAt: string | null
  lastRunAt: string | null
  lastStatus: string | null
  lastSummary: string | null
  lastHash: string | null
  hasWebhook: boolean
  webhookUrl?: string | null
  webhookToken?: string | null
}
export type SmtpSettings = {
  enabled: boolean
  host: string
  port: number
  username: string
  from: string
  hasPassword: boolean
}
export type SearchHit = { kind: string; id: string; title: string; subtitle: string | null }
export type WebhookCreated = { url: string; token: string }
export type AuditStatusFilter = 'ok' | 'error' | 'denied'
export type AuditEntry = {
  id: number
  ts: string
  actor: string
  kind: string
  question: string | null
  sql: string | null
  rows: number | null
  durationMs: number | null
  status: AuditStatusFilter
  detail: unknown
  relations: string[]
}
export type AuditPage = { entries: AuditEntry[]; hasMore: boolean }
export type AuditFilter = { kind?: string; status?: string; search?: string }

export type AutomationStepRun = {
  id: string
  stepId: string
  sort: number
  kind: string
  status: string
  summary: string | null
  facts: unknown
  startedAt: string
  finishedAt: string | null
}
export type AutomationRun = {
  id: string
  triggerKind: string | null
  status: string
  summary: string | null
  notified: boolean
  startedAt: string
  finishedAt: string | null
  steps: AutomationStepRun[]
}

export type MessageStatus = 'pending' | 'done' | 'error' | 'cancelled'
export type Feedback = 1 | -1

export type MessageRecord = {
  id: string
  conversationId: string
  seq: number
  role: 'user' | 'assistant'
  content: string
  status: MessageStatus
  answer: Answer | null
  feedback: Feedback | null
  createdAt: string
}

export type ConversationSummary = { id: string; title: string; updatedAt: string }
export type ConversationDetail = { id: string; title: string; messages: MessageRecord[]; queries: QueryRecord[] }
export type AskStarted = { conversationId: string; userMessageId: string; assistantMessageId: string }

export type AnalystEvent =
  | { type: 'step'; step: Step }
  | { type: 'token'; text: string }
  | { type: 'done'; status: MessageStatus }
export type AnalystEventPayload = { conversationId: string; messageId: string; event: AnalystEvent }

export const DATA_EXTENSIONS = ['csv', 'tsv', 'parquet', 'json', 'jsonl', 'xlsx'] as const

export const api = {
  appStatus: () => call<AppStatus>('app_status'),
  runInBackground: () => call<void>('app_run_in_background'),
  forceQuit: () => call<void>('app_force_quit'),
  notifyTest: () => call<void>('notify_test'),
  notifyOpenSettings: () => call<void>('notify_open_settings'),
  completeOnboarding: () => call<void>('complete_onboarding'),
  searchWorkspace: (query: string) => call<SearchHit[]>('search_workspace', { query }),

  workspaces: () => call<WorkspaceList>('list_workspaces'),
  createWorkspace: (name: string) => call<WorkspaceList>('create_workspace', { name }),
  switchWorkspace: (id: string) => call<WorkspaceList>('switch_workspace', { id }),
  renameWorkspace: (id: string, name: string) => call<WorkspaceList>('rename_workspace', { id, name }),
  deleteWorkspace: (id: string) => call<WorkspaceList>('delete_workspace', { id }),

  connections: () => call<Connection[]>('list_connections'),
  addFileSource: (path: string, recursive = false) => call<AddSourceResult>('add_file_source', { path, recursive }),
  refreshConnection: (id: string) => call<SyncReport>('refresh_connection', { id }),
  removeConnection: (id: string) => call<void>('remove_connection', { id }),
  connectionDependents: (id: string) => call<Dependent[]>('connection_dependents', { id }),
  testDbConnection: (config: DbConfig) => call<string>('test_db_connection', { config }),
  saveDbConnection: (config: DbConfig) => call<Connection>('save_db_connection', { config }),
  exploreDbConnection: (id: string) => call<ExploredTable[]>('explore_db_connection', { id }),
  registerDbTables: (id: string, sourceRefs: string[]) => call<SyncReport>('register_db_tables', { id, sourceRefs }),

  tables: () => call<TableRecord[]>('list_tables'),
  table: (id: string) => call<TableDetail>('get_table', { id }),
  renameTable: (id: string, name: string) => call<TableRecord>('rename_table', { id, name }),
  describeTable: (id: string, description: string) => call<void>('describe_table', { id, description }),
  describeColumn: (tableId: string, column: string, description: string) =>
    call<void>('describe_column', { tableId, column, description }),

  modelCatalog: () => call<ModelCatalog>('llm_catalog'),
  downloadModel: (modelId: string) => call<void>('llm_download', { modelId }),
  cancelDownload: () => call<void>('llm_cancel_download'),
  deleteModel: (modelId: string) => call<void>('llm_delete_model', { modelId }),
  benchmarkModel: (modelId: string) => call<Benchmark>('llm_benchmark', { modelId }),
  stopModel: () => call<void>('llm_stop'),
  setTaskRouting: (routing: TaskRouting) => call<TaskRouting>('set_task_routing', { routing }),

  ask: (question: string, conversationId: string | null) => call<AskStarted>('ask', { question, conversationId }),
  cancelAnswer: (messageId: string) => call<void>('cancel_answer', { messageId }),
  conversations: (search: string) => call<ConversationSummary[]>('list_conversations', { search: search || null }),
  conversation: (id: string) => call<ConversationDetail>('get_conversation', { id }),
  deleteConversation: (id: string) => call<void>('delete_conversation', { id }),
  renameConversation: (id: string, title: string) => call<void>('rename_conversation', { id, title }),
  exportConversation: (id: string, path: string) => call<void>('export_conversation', { id, path }),
  rerunQuery: (id: string) => call<QueryRecord>('rerun_query', { id }),
  setFeedback: (messageId: string, feedback: Feedback | null) => call<void>('set_feedback', { messageId, feedback }),
  starterQuestions: () => call<string[]>('starter_questions'),
  completionSources: () => call<CompletionSources>('completion_sources'),

  agents: () => call<Agent[]>('list_agents'),
  createAgent: (name: string, description: string) => call<Agent>('create_agent', { name, description }),
  updateAgent: (agent: Pick<Agent, 'id' | 'name' | 'description' | 'modelOverrides' | 'assignedTables'>) =>
    call<Agent>('update_agent', {
      id: agent.id,
      name: agent.name,
      description: agent.description,
      modelOverrides: agent.modelOverrides,
      assignedTables: agent.assignedTables,
    }),
  deleteAgent: (id: string) => call<void>('delete_agent', { id }),
  rules: (agentId: string) => call<Rule[]>('list_rules', { agentId }),
  saveRule: (rule: { id?: string; agentId: string; title: string; body: string; tableName: string | null }) =>
    call<Rule>('save_rule', rule),
  deleteRule: (id: string) => call<void>('delete_rule', { id }),
  ruleVersions: (ruleId: string) => call<RuleVersion[]>('list_rule_versions', { ruleId }),
  rollbackRule: (ruleId: string, versionId: string) => call<Rule>('rollback_rule', { ruleId, versionId }),
  examples: (agentId: string) => call<Example[]>('list_examples', { agentId }),
  saveExample: (agentId: string, question: string, sql: string) => call<Example>('save_example', { agentId, question, sql }),
  deleteExample: (id: string) => call<void>('delete_example', { id }),
  saveAnswerExample: (messageId: string) => call<Example>('save_answer_example', { messageId }),
  memories: (agentId: string) => call<Memory[]>('list_memories', { agentId }),
  saveMemory: (memory: { id?: string; agentId: string; body: string }) => call<Memory>('save_memory', memory),
  setMemoryStatus: (id: string, status: MemoryStatus) => call<Memory>('set_memory_status', { id, status }),
  deleteMemory: (id: string) => call<void>('delete_memory', { id }),
  knowledge: (agentId: string) => call<KnowledgeFile[]>('list_knowledge', { agentId }),
  addKnowledge: (agentId: string, path: string) => call<KnowledgeFile>('add_knowledge', { agentId, path }),
  deleteKnowledge: (id: string) => call<void>('delete_knowledge', { id }),

  benchSchema: () => call<SchemaTable[]>('bench_schema'),
  benchFormat: (sql: string) => call<{ sql: string }>('bench_format', { sql }),
  benchRun: (sql: string, params: Record<string, string>) => call<BenchOutcome>('bench_run', { sql, params }),
  benchExplain: (sql: string, params: Record<string, string>) => call<QueryResult>('bench_explain', { sql, params }),
  benchFix: (sql: string, error: string) => call<{ sql: string }>('bench_fix', { sql, error }),
  benchOptimize: (sql: string) => call<{ sql: string }>('bench_optimize', { sql }),
  benchDescribe: (sql: string) => call<{ text: string }>('bench_describe', { sql }),
  benchExport: (sql: string, params: Record<string, string>, format: string, path: string) =>
    call<void>('bench_export', { sql, params, format, path }),
  benchHistory: () => call<HistoryEntry[]>('bench_history'),
  benchSaved: () => call<SavedQuery[]>('bench_saved'),
  benchSaveQuery: (name: string, sql: string, id?: string) => call<SavedQuery>('bench_save_query', { id: id ?? null, name, sql }),
  benchDeleteQuery: (id: string) => call<void>('bench_delete_query', { id }),


  dashboardFolders: () => call<DashboardFolder[]>('dashboard_folders'),
  dashboardCreateFolder: (name: string) => call<DashboardFolder>('dashboard_create_folder', { name }),
  dashboards: () => call<DashboardSummary[]>('dashboard_list'),
  dashboard: (id: string, params: Record<string, string>) => call<LiveDashboard>('dashboard_get', { id, params }),
  dashboardGenerate: (prompt: string) => call<LiveDashboard>('dashboard_generate', { prompt }),
  dashboardAddTile: (dashboardId: string, title: string, sql: string, chart?: string) =>
    call<DashboardTile>('dashboard_add_tile', { dashboardId, title, sql, chart: chart ?? null }),
  dashboardAddTextTile: (dashboardId: string, title: string, markdown: string) =>
    call<DashboardTile>('dashboard_add_text_tile', { dashboardId, title, markdown }),
  dashboardUpdateTextTile: (id: string, title: string, markdown: string) => call<void>('dashboard_update_text_tile', { id, title, markdown }),
  dashboardUpdateTile: (id: string, title: string, sql: string, chart: ChartKind | null) => call<void>('dashboard_update_tile', { id, title, sql, chart }),
  dashboardDeleteTile: (id: string) => call<void>('dashboard_delete_tile', { id }),
  dashboardSaveLayout: (tiles: { id: string; x: number; y: number; w: number; h: number }[]) => call<void>('dashboard_save_layout', { tiles }),
  dashboardUpdate: (dashboard: { id: string; name: string; folderId: string | null; filters: FilterDef[]; refreshSecs: number | null }) =>
    call<DashboardRecord>('dashboard_update', dashboard),
  dashboardDelete: (id: string) => call<void>('dashboard_delete', { id }),
  dashboardDuplicate: (id: string) => call<DashboardRecord>('dashboard_duplicate', { id }),
  dashboardVersions: (id: string) => call<DashboardVersion[]>('dashboard_versions', { id }),
  dashboardRestore: (id: string, versionId: string) => call<DashboardRecord>('dashboard_restore', { id, versionId }),
  dashboardRefresh: (id: string, params: Record<string, string>) => call<LiveDashboard>('dashboard_refresh', { id, params }),

  automations: () => call<Automation[]>('automation_list'),
  automationSave: (draft: {
    id?: string
    name: string
    triggerKind: TriggerKind
    trigger: Record<string, unknown>
    steps: { id?: string; kind: StepKind; config: Record<string, unknown> }[]
    enabled: boolean
    onChangeOnly: boolean
  }) => call<Automation>('automation_save', { draft }),
  automationDelete: (id: string) => call<void>('automation_delete', { id }),
  automationRuns: (id: string) => call<AutomationRun[]>('automation_runs', { id }),
  automationRun: (id: string) => call<void>('automation_run', { id }),
  automationConfirm: (id: string) => call<Automation>('automation_confirm', { id }),
  automationRotateWebhook: (id: string) => call<WebhookCreated>('automation_rotate_webhook', { id }),
  automationPropose: (prompt: string) => call<Automation>('automation_propose', { prompt }),
  automationSetStepSecret: (stepId: string, secret: string) => call<void>('automation_set_step_secret', { stepId, secret }),
  smtpGet: () => call<SmtpSettings>('smtp_get'),
  smtpSave: (draft: { enabled: boolean; host: string; port: number; username: string; from: string; password?: string }) =>
    call<SmtpSettings>('smtp_save', { draft }),

  auditList: (filter: AuditFilter & { beforeTs?: string; beforeId?: number }) =>
    call<AuditPage>('audit_list', {
      kind: filter.kind || null,
      status: filter.status || null,
      search: filter.search || null,
      beforeTs: filter.beforeTs ?? null,
      beforeId: filter.beforeId ?? null,
    }),
  auditExport: (filter: AuditFilter, format: 'json' | 'csv', path: string) =>
    call<{ count: number }>('audit_export', {
      kind: filter.kind || null,
      status: filter.status || null,
      search: filter.search || null,
      format,
      path,
    }),

  datasites: () => call<ArtifactSummary[]>('datasite_list'),
  datasite: (id: string) => call<GeneratedDatasite>('datasite_get', { id }),
  datasiteGenerate: (prompt: string) => call<GeneratedDatasite>('datasite_generate', { prompt }),
  datasiteFromAnswer: (messageId: string) => call<GeneratedDatasite>('datasite_from_answer', { messageId }),
  datasiteDelete: (id: string) => call<void>('datasite_delete', { id }),
  datasiteExport: (id: string, mode: 'static' | 'live', path: string) => call<void>('datasite_export', { id, mode, path }),
  reports: () => call<ArtifactSummary[]>('report_list'),
  report: (id: string) => call<GeneratedReport>('report_get', { id }),
  reportGenerate: (prompt: string) => call<GeneratedReport>('report_generate', { prompt }),
  reportFromAnswer: (messageId: string) => call<GeneratedReport>('report_from_answer', { messageId }),
  reportDelete: (id: string) => call<void>('report_delete', { id }),
  reportExportPdf: (id: string, path: string) => call<void>('report_export_pdf', { id, path }),
  decks: () => call<ArtifactSummary[]>('deck_list'),
  deck: (id: string) => call<GeneratedDeck>('deck_get', { id }),
  deckGenerate: (prompt: string) => call<GeneratedDeck>('deck_generate', { prompt }),
  deckFromAnswer: (messageId: string) => call<GeneratedDeck>('deck_from_answer', { messageId }),
  deckUpdateOutline: (id: string, outline: OutlineSlide[]) => call<DeckRecord>('deck_update_outline', { id, outline }),
  deckDelete: (id: string) => call<void>('deck_delete', { id }),
  deckExportPptx: (id: string, path: string) => call<void>('deck_export_pptx', { id, path }),

  theme: () => call<WorkspaceTheme>('app_theme'),
  setTheme: (brand: string, name: string) => call<WorkspaceTheme>('app_set_theme', { brand, name }),


  governance: () => call<GovernanceStatus>('governance_status'),
  setOffline: (enabled: boolean) => call<boolean>('set_offline', { enabled }),
  setAppLock: (enabled: boolean, passphrase: string, current?: string) => call<boolean>('set_app_lock', { enabled, passphrase, current }),
  lockApp: () => call<boolean>('lock_app'),
  localMcpInfo: () => call<LocalMcpInfo>('local_mcp_info'),
  localMcpRotateToken: () => call<LocalMcpInfo>('local_mcp_rotate_token'),
  unlockWorkspace: (id: string, passphrase: string, remember: boolean) => call<boolean>('unlock_workspace', { id, passphrase, remember }),
  unlockApp: (passphrase: string) => call<boolean>('unlock_app', { passphrase }),
  masks: () => call<ColumnMask[]>('mask_list'),
  maskSave: (draft: { relation: string; columnName: string; strategy: string }) => call<ColumnMask>('mask_save', { draft }),
  maskDelete: (id: string) => call<void>('mask_delete', { id }),
  encryptWorkspace: (passphrase: string, remember = false) => call<boolean>('encrypt_workspace', { passphrase, remember }),
  backupExport: (path: string) => call<void>('backup_export', { path }),
  backupImport: (path: string, name: string, passphrase?: string, remember?: boolean) => call<Workspace>('backup_import', { path, name, passphrase, remember }),
  mcpServers: () => call<McpServer[]>('mcp_list_servers'),
  mcpSaveServer: (draft: { id?: string; name: string; transport: string; command?: string; url?: string; enabled: boolean }) =>
    call<McpServer>('mcp_save_server', { draft }),
  mcpDeleteServer: (id: string) => call<void>('mcp_delete_server', { id }),
  mcpTools: (serverId: string) => call<McpTool[]>('mcp_list_tools', { serverId }),
  mcpRules: (agentId?: string) => call<McpToolRule[]>('mcp_list_rules', { agentId }),
  mcpSetRule: (draft: { agentId: string; serverId: string; toolName: string; allow: boolean }) => call<void>('mcp_set_rule', { draft }),
  mcpDeleteRule: (agentId: string, serverId: string, toolName: string) => call<void>('mcp_delete_rule', { agentId, serverId, toolName }),
}

export function errorMessage(error: unknown) {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return 'Something went wrong'
}
