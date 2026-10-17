import { QueryClient } from '@tanstack/react-query'

export const queryClient = new QueryClient({
  defaultOptions: {
    // All data is local; refetch only when the backend tells us something changed.
    queries: { retry: false, refetchOnWindowFocus: false, staleTime: Infinity },
  },
})

export const queryKeys = {
  status: ['app-status'] as const,
  workspaces: ['workspaces'] as const,
  connections: ['connections'] as const,
  tables: ['tables'] as const,
  table: (id: string) => ['tables', id] as const,
  models: ['model-catalog'] as const,
  conversations: ['conversations'] as const,
  conversationSearch: (search: string) => ['conversations', search] as const,
  conversation: (id: string) => ['conversation', id] as const,
  starters: ['starter-questions'] as const,
  completions: ['completion-sources'] as const,
  agents: ['agents'] as const,
  rules: (id: string) => ['agents', id, 'rules'] as const,
  ruleVersions: (id: string) => ['agents', 'versions', id] as const,
  examples: (id: string) => ['agents', id, 'examples'] as const,
  memories: (id: string) => ['agents', id, 'memories'] as const,
  knowledge: (id: string) => ['agents', id, 'knowledge'] as const,
  benchSchema: ['bench', 'schema'] as const,
  benchHistory: ['bench', 'history'] as const,
  benchSaved: ['bench', 'saved'] as const,
  dashboards: ['dashboards'] as const,
  dashboardFolders: ['dashboards', 'folders'] as const,
  dashboard: (id: string) => ['dashboards', id] as const,
  dashboardVersions: (id: string) => ['dashboards', id, 'versions'] as const,
  datasites: ['datasites'] as const,
  datasite: (id: string) => ['datasites', id] as const,
  reports: ['reports'] as const,
  report: (id: string) => ['reports', id] as const,
  decks: ['decks'] as const,
  deck: (id: string) => ['decks', id] as const,
  theme: ['theme'] as const,
  automations: ['automations'] as const,
  automationRuns: (id: string) => ['automations', id, 'runs'] as const,
  smtp: ['smtp'] as const,
  audit: (kind: string, status: string, search: string) => ['audit', kind, status, search] as const,
  governance: ['governance'] as const,
  masks: ['masks'] as const,
  mcpServers: ['mcp', 'servers'] as const,
  mcpTools: (id: string) => ['mcp', 'tools', id] as const,
  mcpRules: (id?: string) => ['mcp', 'rules', id ?? 'all'] as const,
}

/** Only the queries affected by adding or removing a data source. */
export function invalidateConnectionData() {
  void queryClient.invalidateQueries({ queryKey: queryKeys.connections })
  void queryClient.invalidateQueries({ queryKey: queryKeys.tables })
  void queryClient.invalidateQueries({ queryKey: queryKeys.starters })
  void queryClient.invalidateQueries({ queryKey: queryKeys.completions })
  void queryClient.invalidateQueries({ queryKey: queryKeys.benchSchema })
}

/** Everything scoped to the open workspace; reset when switching workspaces. */
export function invalidateWorkspaceData() {
  void queryClient.invalidateQueries({ queryKey: queryKeys.connections })
  void queryClient.invalidateQueries({ queryKey: queryKeys.tables })
  void queryClient.invalidateQueries({ queryKey: queryKeys.conversations })
  void queryClient.invalidateQueries({ queryKey: queryKeys.starters })
  void queryClient.invalidateQueries({ queryKey: queryKeys.completions })
  void queryClient.invalidateQueries({ queryKey: queryKeys.agents })
  void queryClient.invalidateQueries({ queryKey: queryKeys.dashboards })
  void queryClient.invalidateQueries({ queryKey: queryKeys.datasites })
  void queryClient.invalidateQueries({ queryKey: queryKeys.reports })
  void queryClient.invalidateQueries({ queryKey: queryKeys.decks })
  void queryClient.invalidateQueries({ queryKey: queryKeys.theme })
  void queryClient.invalidateQueries({ queryKey: queryKeys.automations })
  void queryClient.invalidateQueries({ queryKey: queryKeys.smtp })
  void queryClient.invalidateQueries({ queryKey: ['audit'] })
  void queryClient.invalidateQueries({ queryKey: queryKeys.governance })
  void queryClient.invalidateQueries({ queryKey: queryKeys.masks })
  void queryClient.invalidateQueries({ queryKey: queryKeys.mcpServers })
}
