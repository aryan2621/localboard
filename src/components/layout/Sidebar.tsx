import { useEffect, useState, type ReactNode } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import {
  Check,
  Power,
  PowerOff,
  ChevronRight,
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
import { api } from '../../lib/api'
import { lockNow } from '../../lib/lock'
import { iconStroke } from '../../lib/icon'
import { queryKeys } from '../../lib/queryClient'
import { useAppearance, type ThemeMode } from '../../lib/theme'
import { cn } from '../../lib/utils'
import { useWorkspaceAction, useWorkspaces } from '../../lib/workspaces'
import { useNavigation, type AppPage } from '../../stores/navigation'
import { NewWorkspaceDialog } from '../workspaces/NewWorkspaceDialog'
import { UnlockWorkspaceForm } from '../workspaces/UnlockWorkspaceForm'
import { Sheet, SheetBody, SheetContent, SheetHeader } from '../ui/sheet'
import { Input } from '../ui/input'
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

function NavRow({ icon, label, active, onClick, trailing }: { icon: ReactNode; label: string; active: boolean; onClick: () => void; trailing?: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? 'page' : undefined}
      className={cn(
        'relative flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-app-label transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40',
        active
          ? 'bg-accent font-medium text-foreground'
          : 'text-foreground/80 hover:bg-accent/70 hover:text-foreground',
      )}
    >
      {icon}
      <span className="flex-1 truncate text-left">{label}</span>
      {trailing}
    </button>
  )
}

function NavGroup({ label, items, collapsible = false }: { label: string; items: NavItem[]; collapsible?: boolean }) {
  const page = useNavigation((state) => state.page)
  const navigate = useNavigation((state) => state.navigate)
  const hasActive = items.some((item) => item.page === page)
  const [open, setOpen] = useState(!collapsible || hasActive)

  // Following a link (command palette, deep link) opens the group that holds the page.
  useEffect(() => {
    if (hasActive) setOpen(true)
  }, [hasActive])

  return (
    <div className="space-y-0.5">
      {collapsible ? (
        <button
          type="button"
          onClick={() => setOpen((current) => !current)}
          aria-expanded={open}
          className="flex w-full items-center gap-1 rounded-md px-2.5 pb-1 pt-2 text-app-caption font-medium text-muted-foreground transition-colors duration-fast hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
        >
          {label}
          <ChevronRight size={12} strokeWidth={iconStroke} className={cn('transition-transform duration-fast', open && 'rotate-90')} />
        </button>
      ) : (
        <p className="px-2.5 pb-1 pt-2 text-app-caption font-medium text-muted-foreground">{label}</p>
      )}
      {open &&
        items.map(({ page: target, label: itemLabel, icon: Icon }) => (
          <NavRow
            key={target}
            icon={<Icon size={17} strokeWidth={iconStroke} />}
            label={itemLabel}
            active={page === target}
            onClick={() => navigate(target)}
          />
        ))}
    </div>
  )
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
          className="rounded-md text-app-caption text-muted-foreground transition-colors duration-fast hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
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
                'flex h-8 w-full items-center rounded-lg px-2 text-left text-app-caption transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40',
                active ? 'bg-accent text-foreground' : 'text-foreground/75 hover:bg-accent hover:text-foreground',
              )}
            >
              <span className="truncate">{conversation.title}</span>
            </button>
          )
        })}
      </div>
    </div>
  )
}

function AiStatus() {
  const { data: catalog } = useQuery({ queryKey: queryKeys.models, queryFn: api.modelCatalog })
  const openSettings = useNavigation((state) => state.openSettings)
  const active = catalog?.models.find((model) => model.id === catalog.active)
  const ready = Boolean(active && catalog?.runtimeAvailable)

  return (
    <button
      type="button"
      onClick={() => openSettings('models')}
      className="flex h-8 w-full items-center gap-2 rounded-lg px-2 text-app-caption text-muted-foreground transition-colors duration-fast hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
    >
      <Cpu size={14} strokeWidth={iconStroke} className={ready ? 'text-success' : 'text-warning'} />
      <span className="truncate">{ready ? `On this device · ${active?.name ?? ''}` : 'SQL-only mode · no AI model'}</span>
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
              'flex h-11 items-center gap-2.5 rounded-lg text-left transition-colors duration-fast hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40',
              compact ? 'w-11 justify-center' : 'w-full px-2',
            )}
          >
            <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-brand text-app-caption font-semibold text-brand-foreground">
              {(active?.name ?? '?').charAt(0).toUpperCase()}
            </span>
            {!compact && (
              <>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-app-label font-medium text-foreground">{active?.name ?? 'No workspace'}</span>
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
              <span className="flex-1 truncate">{workspace.name}</span>
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
          <DropdownMenuItem onSelect={() => void api.runInBackground()}>
            <Power strokeWidth={iconStroke} /> Quit (keep running in background)
          </DropdownMenuItem>
          <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => void api.forceQuit()}>
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

export function Sidebar() {
  const page = useNavigation((state) => state.page)
  const navigate = useNavigation((state) => state.navigate)

  return (
    <aside className="flex w-[296px] shrink-0 flex-col border-r border-border bg-sidebar">
      <div className="mac-window-space mac-drag-region shrink-0" />
      <div className="flex items-center gap-2.5 px-6 pb-5 pt-6">
        <img src="/app-icon.png" alt="" className="h-[26px] w-[26px] rounded-md" />
        <h1 className="font-serif text-app-title text-foreground">LocalBoard</h1>
      </div>
      <nav className="flex min-h-0 flex-1 flex-col px-5">
        <div className="space-y-0.5">
          {TOP_NAV.map(({ page: target, label, icon: Icon }) => (
            <NavRow
              key={target}
              icon={<Icon size={17} strokeWidth={iconStroke} />}
              label={label}
              active={page === target}
              onClick={() => navigate(target)}
            />
          ))}
        </div>
        <NavGroup label="Data" items={DATA_NAV} />
        <NavGroup label="Workspace" items={WORKSPACE_NAV} />
        <ChatHistory />
      </nav>
      <div className="space-y-1 border-t border-border px-5 py-3">
        <AiStatus />
        <WorkspaceMenu />
      </div>
    </aside>
  )
}
