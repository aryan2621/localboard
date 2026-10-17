import { lazy, Suspense, useEffect, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { Loader2 } from 'lucide-react'
import { toast } from 'sonner'
import { Toaster } from './components/ui/sonner'
import { TooltipProvider } from './components/ui/tooltip'
import { api, errorMessage } from './lib/api'
import { useBackendEvents } from './lib/events'
import { iconStroke } from './lib/icon'
import { queryClient, queryKeys } from './lib/queryClient'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { UnlockWorkspaceForm } from './components/workspaces/UnlockWorkspaceForm'
import { useLockShortcut } from './lib/lock'
import { useAddSources } from './lib/sources'
import { useAppearanceSync } from './lib/theme'
import { useNavigation } from './stores/navigation'

const CommandPalette = lazy(() => import('./components/layout/CommandPalette').then((m) => ({ default: m.CommandPalette })))
const Sidebar = lazy(() => import('./components/layout/Sidebar').then((m) => ({ default: m.Sidebar })))
const SettingsModal = lazy(() => import('./components/settings/SettingsModal').then((m) => ({ default: m.SettingsModal })))

const AgentsPage = lazy(() => import('./pages/Agents').then((m) => ({ default: m.AgentsPage })))
const ChatPage = lazy(() => import('./pages/Chat').then((m) => ({ default: m.ChatPage })))
const AuditPage = lazy(() => import('./pages/Audit').then((m) => ({ default: m.AuditPage })))
const AutomationsPage = lazy(() => import('./pages/Automations').then((m) => ({ default: m.AutomationsPage })))
const DashboardsPage = lazy(() => import('./pages/Dashboards').then((m) => ({ default: m.DashboardsPage })))
const DatasitesPage = lazy(() => import('./pages/Datasites').then((m) => ({ default: m.DatasitesPage })))
const QueryBenchPage = lazy(() => import('./pages/QueryBench').then((m) => ({ default: m.QueryBenchPage })))
const ConnectionsPage = lazy(() => import('./pages/Connections').then((m) => ({ default: m.ConnectionsPage })))
const ChatHistoryPage = lazy(() => import('./pages/ChatHistory').then((m) => ({ default: m.ChatHistoryPage })))
const HomePage = lazy(() => import('./pages/Home').then((m) => ({ default: m.HomePage })))
const Onboarding = lazy(() => import('./pages/Onboarding').then((m) => ({ default: m.Onboarding })))
const SchemaExplorerPage = lazy(() => import('./pages/SchemaExplorer').then((m) => ({ default: m.SchemaExplorerPage })))

function PageFallback() {
  return (
    <div className="flex h-full items-center justify-center" role="status" aria-label="Loading">
      <Loader2 size={20} strokeWidth={iconStroke} className="animate-spin text-muted-foreground" />
    </div>
  )
}

function isTauri() {
  return Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)
}

/** Files or folders dropped anywhere on the window become sources; folders are watched recursively. */
function useWindowDrop() {
  const [dragging, setDragging] = useState(false)
  const add = useAddSources()
  const navigate = useNavigation((state) => state.navigate)
  const { mutate } = add

  useEffect(() => {
    if (!isTauri()) return
    let active = true
    let unlisten: (() => void) | undefined
    void import('@tauri-apps/api/webview')
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent(({ payload }) => {
          if (payload.type === 'enter') setDragging(true)
          else if (payload.type === 'leave') setDragging(false)
          else if (payload.type === 'drop') {
            setDragging(false)
            if (payload.paths.length === 0) return
            navigate('connections')
            mutate({ paths: payload.paths, recursive: true })
          }
        }),
      )
      .then((stop) => {
        if (active) unlisten = stop
        else stop()
      })
      .catch((failure) => console.error('Drag and drop is unavailable:', failure))
    return () => {
      active = false
      unlisten?.()
    }
  }, [mutate, navigate])

  return dragging
}

function OfflineBanner() {
  const { data: status } = useQuery({ queryKey: queryKeys.status, queryFn: api.appStatus })
  if (!status?.offline) return null
  return (
    <div className="mx-8 mt-2 rounded-lg border border-border bg-card px-3 py-2 text-app-caption text-foreground">
      Offline mode is on. Downloads, SMTP and remote MCP servers are blocked.
    </div>
  )
}

function LockScreen() {
  const [passphrase, setPassphrase] = useState('')
  const unlock = useMutation({
    mutationFn: () => api.unlockApp(passphrase),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.status })
      void queryClient.invalidateQueries({ queryKey: queryKeys.governance })
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <div className="flex h-full items-center justify-center p-8">
      <form
        className="w-full max-w-sm space-y-4"
        onSubmit={(event) => {
          event.preventDefault()
          unlock.mutate()
        }}
      >
        <h1 className="font-serif text-app-page text-foreground">Unlock LocalBoard</h1>
        <p className="text-app-label text-muted-foreground">LocalBoard is locked. Enter your app lock passphrase.</p>
        <Input type="password" value={passphrase} onChange={(event) => setPassphrase(event.target.value)} aria-label="Passphrase" autoFocus />
        <Button type="submit" disabled={!passphrase || unlock.isPending}>
          Unlock
        </Button>
      </form>
    </div>
  )
}

function WorkspacePassphraseScreen({ id }: { id: string }) {
  const { data } = useQuery({ queryKey: queryKeys.workspaces, queryFn: api.workspaces })
  const name = data?.workspaces.find((workspace) => workspace.id === id)?.name
  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="w-full max-w-sm space-y-4">
        <h1 className="font-serif text-app-page text-foreground">Unlock workspace</h1>
        <p className="text-app-label text-muted-foreground">{name ? `${name} is encrypted.` : 'This workspace is encrypted.'} Enter its passphrase to open it.</p>
        <UnlockWorkspaceForm id={id} />
      </div>
    </div>
  )
}

function Workspace() {
  const page = useNavigation((state) => state.page)
  const dragging = useWindowDrop()
  useLockShortcut()

  return (
    <div className="flex h-full min-h-0 w-full overflow-hidden">
      <Suspense fallback={null}><Sidebar /></Suspense>
      <main className="min-w-0 flex-1 overflow-y-auto">
        <div className="mac-window-space mac-drag-region" data-tauri-drag-region />
        <OfflineBanner />
        <Suspense fallback={<PageFallback />}>
          {page === 'home' && <HomePage />}
          {page === 'chat' && <ChatPage />}
          {page === 'history' && <ChatHistoryPage />}
          {page === 'connections' && <ConnectionsPage />}
          {page === 'schema' && <SchemaExplorerPage />}
          {page === 'agents' && <AgentsPage />}
          {page === 'bench' && <QueryBenchPage />}
          {page === 'dashboards' && <DashboardsPage />}
          {page === 'datasites' && <DatasitesPage />}
          {page === 'automations' && <AutomationsPage />}
          {page === 'audit' && <AuditPage />}
        </Suspense>
      </main>
      {dragging && <div className="pointer-events-none fixed inset-2 z-30 rounded-xl border-2 border-brand bg-brand/5" />}
      <Suspense fallback={null}>
        <SettingsModal />
        <CommandPalette />
      </Suspense>
    </div>
  )
}

export default function App() {
  useAppearanceSync()
  useBackendEvents()
  const { data: status, error } = useQuery({
    queryKey: queryKeys.status,
    queryFn: api.appStatus,
    refetchInterval: (query) => (query.state.data?.ready ? false : 400),
  })

  return (
    <TooltipProvider delayDuration={250}>
      {error ? (
        <div className="flex h-full items-center justify-center p-8 text-app-label text-destructive">{errorMessage(error)}</div>
      ) : !status?.ready ? (
        <div className="flex h-full items-center justify-center" role="status" aria-label="Starting LocalBoard">
          <Loader2 size={20} strokeWidth={iconStroke} className="animate-spin text-muted-foreground" />
        </div>
      ) : status.onboardingCompleted ? (
        status.locked ? (
          <LockScreen />
        ) : status.needsPassphrase ? (
          <WorkspacePassphraseScreen id={status.needsPassphrase} />
        ) : (
          <Workspace />
        )
      ) : (
        <Suspense fallback={<PageFallback />}>
          <Onboarding />
        </Suspense>
      )}
      <Toaster />
    </TooltipProvider>
  )
}
