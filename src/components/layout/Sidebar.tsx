import { useEffect, useState, type ReactNode } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import {
  Check,
  Power,
  PowerOff,
  Lock,
  ChevronsUpDown,
  Cpu,
  Bot,
  Database,
  Globe,
  LayoutDashboard,
  Timer,
  ScrollText,
  SquareTerminal,
  MessageSquarePlus,
  Monitor,
  Moon,
  Plus,
  Search,
  Settings,
  Sun,
  Table2,
} from 'lucide-react'
import { toast } from 'sonner'
import { api, errorMessage } from '../../lib/api'
import { lockNow } from '../../lib/lock'
import { iconStroke } from '../../lib/icon'
import { queryKeys } from '../../lib/queryClient'
import { useAppearance, type ThemeMode } from '../../lib/theme'
import { cn } from '../../lib/utils'
import { useWorkspaceAction, useWorkspaces } from '../../lib/workspaces'
import { useNavigation, type AppPage } from '../../stores/navigation'
import { useAiReady } from '../analyst/Composer'
import { NewWorkspaceDialog } from '../workspaces/NewWorkspaceDialog'
import { UnlockWorkspaceForm } from '../workspaces/UnlockWorkspaceForm'
import { Sheet, SheetBody, SheetContent, SheetHeader } from '../ui/sheet'
import { Input } from '../ui/input'
import { Hint } from '../ui/tooltip'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '../ui/dropdown-menu'

type NavItem = { page: AppPage; label: string; icon: typeof Database }

const TOP_NAV: NavItem[] = [
  { page: 'home', label: 'New chat', icon: MessageSquarePlus },
  { page: 'dashboards', label: 'Dashboards', icon: LayoutDashboard },
  { page: 'datasites', label: 'Publish', icon: Globe },
  { page: 'automations', label: 'Automations', icon: Timer },
]

const DATA_NAV: NavItem[] = [
  { page: 'connections', label: 'Connections', icon: Database },
  { page: 'schema', label: 'Schema', icon: Table2 },
  { page: 'bench', label: 'Query Bench', icon: SquareTerminal },
]

const WORKSPACE_NAV: NavItem[] = [
  { page: 'agents', label: 'Analysts', icon: Bot },
  { page: 'audit', label: 'Audit', icon: ScrollText },
]

const THEME_LABEL: Record<ThemeMode, { label: string; icon: ReactNode }> = {
  light: { label: 'Light', icon: <Sun strokeWidth={iconStroke} /> },
  dark: { label: 'Dark', icon: <Moon strokeWidth={iconStroke} /> },
  system: { label: 'Match system', icon: <Monitor strokeWidth={iconStroke} /> },
}

const SEARCH_DEBOUNCE_MS = 200

function ChatHistory() {
  const [draft, setDraft] = useState('')
  const [search, setSearch] = useState('')
  const { data: conversations = [] } = useQuery({
    queryKey: queryKeys.conversationSearch(search),
    queryFn: () => api.conversations(search),
    placeholderData: keepPreviousData,
  })
  const conversationId = useNavigation((state) => state.conversationId)
  const page = useNavigation((state) => state.page)
  const openConversation = useNavigation((state) => state.openConversation)
  const navigate = useNavigation((state) => state.navigate)

  useEffect(() => {
    const timer = window.setTimeout(() => setSearch(draft.trim()), SEARCH_DEBOUNCE_MS)
    return () => window.clearTimeout(timer)
  }, [draft])

  // A first-time user has no chats: show nothing rather than an empty search box and a hint.
  if (conversations.length === 0 && !draft) return <div className="flex-1" />

  return (
    <div className="mt-4 flex min-h-0 flex-1 flex-col border-t border-border pt-3">
      <div className="flex items-center justify-between px-2 pb-1.5">
        <p className="text-app-caption font-medium text-muted-foreground">Recent chats</p>
        <button
          type="button"
          onClick={() => navigate('history')}
          className="rounded-md text-app-caption text-muted-foreground transition-colors duration-fast hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          View all
        </button>
      </div>
      <div className="relative px-0.5 pb-2">
        <Search size={14} strokeWidth={iconStroke} className="pointer-events-none absolute left-2.5 top-2 text-muted-foreground" />
        <Input
          aria-label={'Search chats'}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder={'Search chats'}
          className="h-8 bg-transparent pl-7 text-app-caption"
        />
      </div>
      <div className="min-h-0 flex-1 space-y-0.5 overflow-y-auto">
        {conversations.length === 0 && (
          <p className="px-2 py-1 text-app-caption text-muted-foreground">No chats match.</p>
        )}
        {conversations.map((conversation) => {
          const active = page === 'chat' && conversationId === conversation.id
          return (
            <button
              key={conversation.id}
              type="button"
              onClick={() => openConversation(conversation.id)}
              title={conversation.title}
              aria-current={active ? 'page' : undefined}
              className={cn(
                'flex h-8 w-full items-center rounded-lg px-2 text-left text-app-caption transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
                active ? 'bg-accent text-foreground' : 'text-foreground/75 hover:bg-accent hover:text-foreground',
              )}
            >
              <span className="truncate" title={conversation.title}>{conversation.title}</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

function AiStatus() {
  const openSettings = useNavigation((state) => state.openSettings)
  const ready = useAiReady()

  return (
    <button
      type="button"
      onClick={() => openSettings('models')}
      className="flex h-8 w-full items-center gap-2 rounded-lg px-2 text-app-caption text-muted-foreground transition-colors duration-fast hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <Cpu size={14} strokeWidth={iconStroke} className={ready ? 'text-success' : 'text-warning'} />
      <span className="truncate">{ready ? 'AI provider ready' : 'SQL-only mode · no AI provider'}</span>
    </button>
  )
}

function WorkspaceMenu({ compact = false }: { compact?: boolean }) {
  const { data } = useWorkspaces()
  const action = useWorkspaceAction()
  const openSettings = useNavigation((state) => state.openSettings)
  const theme = useAppearance((state) => state.theme)
  const cycleTheme = useAppearance((state) => state.cycleTheme)
  const [creating, setCreating] = useState(false)
  const [unlocking, setUnlocking] = useState<{ id: string; name: string } | null>(null)
  const active = data?.workspaces.find((workspace) => workspace.id === data.activeId)

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            aria-label={compact ? `Workspace: ${active?.name ?? 'none'}` : 'Workspaces'}
            className={cn(
              'flex h-11 items-center gap-2.5 rounded-lg text-left transition-colors duration-fast hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
              compact ? 'w-11 justify-center' : 'w-full px-2',
            )}
          >
            <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-brand text-app-caption font-semibold text-brand-foreground">
              {(active?.name ?? '?').charAt(0).toUpperCase()}
            </span>
            {!compact && (
              <>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-app-label font-medium text-foreground" title={active?.name}>{active?.name ?? 'No workspace'}</span>
                  <span className="block text-app-caption text-muted-foreground">{'Workspace'}</span>
                </span>
                <ChevronsUpDown size={15} strokeWidth={iconStroke} className="text-muted-foreground" />
              </>
            )}
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" side="top" className="w-60">
          <DropdownMenuLabel>{'Workspaces'}</DropdownMenuLabel>
          {data?.workspaces.map((workspace) => (
            <DropdownMenuItem
              key={workspace.id}
              onSelect={() =>
                workspace.id !== data.activeId &&
                action.mutate(async () => {
                  try {
                    return await api.switchWorkspace(workspace.id)
                  } catch (failure) {
                    if (workspace.encrypted) setUnlocking({ id: workspace.id, name: workspace.name })
                    throw failure
                  }
                })
              }
            >
              <span className="flex-1 truncate" title={workspace.name}>{workspace.name}</span>
              {workspace.id === data.activeId && <Check strokeWidth={iconStroke} />}
            </DropdownMenuItem>
          ))}
          <DropdownMenuItem onSelect={() => setCreating(true)}>
            <Plus strokeWidth={iconStroke} /> {'New workspace'}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => openSettings()}>
            <Settings strokeWidth={iconStroke} /> {'Settings'}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => void lockNow()}>
            <Lock strokeWidth={iconStroke} /> Lock now
          </DropdownMenuItem>
          <DropdownMenuItem
            onSelect={(event) => {
              event.preventDefault()
              cycleTheme()
            }}
          >
            {THEME_LABEL[theme].icon} {`Theme: ${THEME_LABEL[theme].label}`}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => api.runInBackground().catch((failure) => toast.error(errorMessage(failure)))}>
            <Power strokeWidth={iconStroke} /> Quit (keep running in background)
          </DropdownMenuItem>
          <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => api.forceQuit().catch((failure) => toast.error(errorMessage(failure)))}>
            <PowerOff strokeWidth={iconStroke} /> Force quit (stop everything)
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <NewWorkspaceDialog open={creating} onOpenChange={setCreating} />
      <Sheet open={unlocking !== null} onOpenChange={(open) => !open && setUnlocking(null)}>
        <SheetContent>
          <SheetHeader title={`Unlock ${unlocking?.name ?? ''}`} description="This workspace is encrypted. Enter its passphrase to open it." />
          <SheetBody>{unlocking && <UnlockWorkspaceForm id={unlocking.id} onDone={() => setUnlocking(null)} />}</SheetBody>
        </SheetContent>
      </Sheet>
    </>
  )
}

const RAIL: NavItem[][] = [TOP_NAV, DATA_NAV, WORKSPACE_NAV]

/** Slim icon rail: every page one click away, labels on hover. */
export function Sidebar() {
  const page = useNavigation((state) => state.page)
  const navigate = useNavigation((state) => state.navigate)

  return (
    <aside className="flex w-12 shrink-0 flex-col items-center border-r border-border bg-sidebar">
      <div className="mac-window-space mac-drag-region w-full shrink-0" />
      <img src="/app-icon.png" alt="LocalBoard" className="my-3 h-5 w-5 rounded-md" />
      <nav aria-label="Pages" className="flex min-h-0 flex-1 flex-col items-center gap-3 overflow-y-auto py-1">
        {RAIL.map((group, index) => (
          <div key={index} className={cn('flex flex-col items-center gap-0.5', index > 0 && 'border-t border-border pt-3')}>
            {group.map(({ page: target, label, icon: Icon }) => (
              <Hint key={target} text={label} side="right">
                <button
                  type="button"
                  aria-label={label}
                  aria-current={page === target ? 'page' : undefined}
                  onClick={() => navigate(target)}
                  className={cn(
                    'flex h-8 w-8 items-center justify-center rounded-md transition-colors duration-fast ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
                    page === target ? 'bg-accent text-foreground' : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground',
                  )}
                >
                  <Icon size={16} strokeWidth={iconStroke} />
                </button>
              </Hint>
            ))}
          </div>
        ))}
      </nav>
      <div className="py-2">
        <WorkspaceMenu compact />
      </div>
    </aside>
  )
}

export { ChatHistory, AiStatus }
