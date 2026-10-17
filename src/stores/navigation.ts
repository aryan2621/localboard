import { create } from 'zustand'

export type AppPage = 'home' | 'chat' | 'history' | 'connections' | 'schema' | 'agents' | 'bench' | 'dashboards' | 'datasites' | 'automations' | 'audit'
export type ArtifactTab = 'sites' | 'reports' | 'decks'
export type SettingsSectionId = 'general' | 'brand' | 'email' | 'models' | 'workspaces' | 'security' | 'integrations'

type NavigationState = {
  page: AppPage
  selectedTableId: string | null
  /** Connection whose tables the Schema list is filtered to. */
  schemaSource: string | null
  conversationId: string | null
  dashboardId: string | null
  datasiteId: string | null
  reportId: string | null
  deckId: string | null
  automationId: string | null
  benchSql: string | null
  artifactTab: ArtifactTab
  settingsOpen: boolean
  settingsSection: SettingsSectionId
  /** Page shown before the current one; list-and-detail pages go back to it. */
  previousPage: AppPage
  /** List-and-detail pages currently showing their list in place of the app sidebar. */
  navigate: (page: AppPage) => void
  openTable: (id: string) => void
  openSchemaFor: (connectionId: string) => void
  openConversation: (id: string) => void
  openDashboard: (id: string | null) => void
  openDatasite: (id: string | null) => void
  openReport: (id: string | null) => void
  openDeck: (id: string | null) => void
  openAutomation: (id: string | null) => void
  openBench: (sql?: string) => void
  setArtifactTab: (tab: ArtifactTab) => void
  openSettings: (section?: SettingsSectionId) => void
  closeSettings: () => void
  setSettingsSection: (section: SettingsSectionId) => void
  resetForWorkspace: () => void
}

export const useNavigation = create<NavigationState>((set, get) => ({
  page: 'home',
  selectedTableId: null,
  schemaSource: null,
  conversationId: null,
  dashboardId: null,
  datasiteId: null,
  reportId: null,
  deckId: null,
  automationId: null,
  benchSql: null,
  artifactTab: 'sites',
  settingsOpen: false,
  settingsSection: 'general',
  previousPage: 'home',
  // Opening Schema from the sidebar always lands on the table list, not the last table viewed.
  navigate: (page) => set(page === 'schema' ? { page, selectedTableId: null, schemaSource: null } : { page }),
  openTable: (id) => set({ page: 'schema', selectedTableId: id }),
  openSchemaFor: (connectionId) => set({ page: 'schema', selectedTableId: null, schemaSource: connectionId }),
  openConversation: (id) => set({ page: 'chat', conversationId: id }),
  openDashboard: (id) => set({ page: 'dashboards', dashboardId: id }),
  openDatasite: (id) => set({ page: 'datasites', artifactTab: 'sites', datasiteId: id, reportId: null, deckId: null }),
  openReport: (id) => set({ page: 'datasites', artifactTab: 'reports', reportId: id, datasiteId: null, deckId: null }),
  openDeck: (id) => set({ page: 'datasites', artifactTab: 'decks', deckId: id, datasiteId: null, reportId: null }),
  openAutomation: (id) => set({ page: 'automations', automationId: id }),
  openBench: (sql) => set({ page: 'bench', benchSql: sql ?? null }),
  setArtifactTab: (artifactTab) => set({ page: 'datasites', artifactTab, datasiteId: null, reportId: null, deckId: null }),
  openSettings: (section = 'general') => set({ settingsOpen: true, settingsSection: section }),
  closeSettings: () => set({ settingsOpen: false }),
  setSettingsSection: (section) => set({ settingsSection: section }),
  resetForWorkspace: () =>
    set({ page: 'home', selectedTableId: null, schemaSource: null, conversationId: null, dashboardId: null, datasiteId: null, reportId: null, deckId: null, benchSql: null, artifactTab: 'sites' }),
}))

// Remember where the user came from, so a page that replaces the sidebar can send them back there.
useNavigation.subscribe((state, prev) => {
  if (state.page !== prev.page) useNavigation.setState({ previousPage: prev.page })
})
