import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { AreaChart, BarChart3, BarChartBig, Check, Code2, Filter, Grid3x3, Hash, LayoutDashboard, LineChart, Maximize2, PieChart, Plus, RefreshCw, SlidersHorizontal, Table2, Trash2, X } from 'lucide-react'
import { toast } from 'sonner'
import { Card } from '../../components/ui/card'
import { GenerationSteps } from '../../components/analyst/StepsPanel'
import { ResultChart } from '../../components/analyst/ResultChart'
import { ConfirmDialog, DetailHeader, EmptyState, FormField, PageContainer, PageHeader, PageLoader, RowMenu, SearchInput, SectionHeader, SegmentedControl, StatusBadge } from '../../components/layout/ui'
import { ConfirmAction } from '../../components/ui/alert-dialog'
import { Alert, AlertDescription } from '../../components/ui/alert'
import { Button } from '../../components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuSub, DropdownMenuSubContent, DropdownMenuSubTrigger, DropdownMenuTrigger } from '../../components/ui/dropdown-menu'
import { Sheet, SheetBody, SheetContent, SheetFooter, SheetHeader } from '../../components/ui/sheet'
import { Label } from '../../components/ui/label'
import { cn } from '../../lib/utils'
import { Input } from '../../components/ui/input'
import { PaginationBar } from '../../components/ui/pagination'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../../components/ui/select'
import { Textarea } from '../../components/ui/textarea'
import { Hint } from '../../components/ui/tooltip'
import { api, errorMessage, type ChartKind, type DashboardSummary, type LiveDashboard, type LiveTile } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { usePagination } from '../../lib/usePagination'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { useGeneration } from '../../stores/generation'
import { relativeTime } from '../../lib/time'
import { IconTile } from '../../components/layout/patterns'
import { useNavigation } from '../../stores/navigation'
import 'react-resizable/css/styles.css'

/** Renders a safe markdown subset as React nodes, so no raw HTML is ever injected. */
function inlineMarkdown(text: string, keyBase: string): ReactNode[] {
  const out: ReactNode[] = []
  const pattern = /(\*\*[^*]+\*\*|\*[^*]+\*|`[^`]+`|\[[^\]]+\]\(https?:\/\/[^)\s]+\))/g
  let last = 0
  let n = 0
  for (const match of text.matchAll(pattern)) {
    const index = match.index ?? 0
    if (index > last) out.push(text.slice(last, index))
    const token = match[0]
    const key = `${keyBase}-${n++}`
    if (token.startsWith('**')) out.push(<strong key={key}>{token.slice(2, -2)}</strong>)
    else if (token.startsWith('`')) out.push(<code key={key} className="rounded bg-muted px-1 font-mono text-app-caption">{token.slice(1, -1)}</code>)
    else if (token.startsWith('[')) {
      const [, label, href] = token.match(/^\[([^\]]+)\]\((.+)\)$/) ?? []
      out.push(
        <a key={key} href={href} target="_blank" rel="noreferrer noopener" className="text-brand underline">
          {label}
        </a>,
      )
    } else out.push(<em key={key}>{token.slice(1, -1)}</em>)
    last = index + token.length
  }
  if (last < text.length) out.push(text.slice(last))
  return out
}

function Markdown({ source }: { source: string }) {
  const blocks: ReactNode[] = []
  let list: string[] = []
  const flush = () => {
    if (!list.length) return
    const items = list
    blocks.push(
      <ul key={`ul-${blocks.length}`} className="list-disc pl-5 text-app-label">
        {items.map((item, i) => (
          <li key={i}>{inlineMarkdown(item, `li-${blocks.length}-${i}`)}</li>
        ))}
      </ul>,
    )
    list = []
  }
  source.split('\n').forEach((line, i) => {
    const bullet = line.match(/^\s*[-*]\s+(.*)$/)
    if (bullet) {
      list.push(bullet[1])
      return
    }
    flush()
    const heading = line.match(/^(#{1,3})\s+(.*)$/)
    if (heading) {
      const size = ['text-app-title', 'text-app-title', 'text-app-label font-semibold'][heading[1].length - 1]
      blocks.push(
        <p key={i} className={`${size} font-serif font-medium`}>
          {inlineMarkdown(heading[2], `h-${i}`)}
        </p>,
      )
    } else if (line.trim()) {
      blocks.push(
        <p key={i} className="text-app-label">
          {inlineMarkdown(line, `p-${i}`)}
        </p>,
      )
    }
  })
  flush()
  return <div className="space-y-2">{blocks}</div>
}

function TextTile({ id, title, markdown, onSaved }: { id: string; title: string; markdown: string; onSaved: () => void }) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(markdown)
  const save = useMutation({
    mutationFn: () => api.dashboardUpdateTextTile(id, title, draft),
    onSuccess: () => {
      setEditing(false)
      onSaved()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  if (editing) {
    return (
      <div className="space-y-2">
        <Textarea value={draft} onChange={(event) => setDraft(event.target.value)} rows={5} aria-label="Note markdown" />
        <div className="flex gap-2">
          <Button size="sm" onClick={() => save.mutate()} disabled={save.isPending}>
            Save
          </Button>
          <Button size="sm" variant="ghost" onClick={() => { setDraft(markdown); setEditing(false) }}>
            Cancel
          </Button>
        </div>
      </div>
    )
  }
  return (
    <div className="space-y-2" onDoubleClick={() => { setDraft(markdown); setEditing(true) }}>
      {markdown.trim() ? <Markdown source={markdown} /> : <p className="text-app-label text-muted-foreground">Empty note. Double-click to edit.</p>}
      <Button size="sm" variant="ghost" onClick={() => { setDraft(markdown); setEditing(true) }}>
        Edit
      </Button>
    </div>
  )
}

/** Radix Select items cannot be empty, so "no folder" uses a sentinel. */
const DASHBOARD_TIPS = [
  { label: 'What to measure', example: 'revenue, order count, refund rate' },
  { label: 'How to split it', example: 'by month, by region, by product' },
  { label: 'Time range', example: 'last 12 months, this year' },
  { label: 'Rankings', example: 'top 10 customers by spend' },
]

const DASHBOARD_EXAMPLES = [
  'Sales overview: revenue by month, orders by region and top 10 customers',
  'Refunds: refund count and amount by month, and refund rate by region',
  'Customers: new customers per month and customers by segment',
]

const NO_FOLDER = 'none'

type DashSort = 'recent' | 'name'

const DASH_SORTS: { value: DashSort; label: string }[] = [
  { value: 'recent', label: 'Recent' },
  { value: 'name', label: 'Name' },
]

function DashboardGroups({
  dashboards,
  folders,
  onOpen,
}: {
  dashboards: DashboardSummary[]
  folders: { id: string; name: string }[]
  onOpen: (id: string) => void
}) {
  const [search, setSearch] = useState('')
  const [sort, setSort] = useState<DashSort>('recent')
  const term = search.trim().toLowerCase()
  const shown = useMemo(() => {
    const filtered = term ? dashboards.filter((item) => item.name.toLowerCase().includes(term)) : dashboards
    return [...filtered].sort((a, b) => (sort === 'name' ? a.name.localeCompare(b.name) : b.updatedAt.localeCompare(a.updatedAt)))
  }, [dashboards, term, sort])
  const groups = useMemo(() => {
    const named = folders.filter((folder) => shown.some((item) => item.folderId === folder.id)).map((folder) => ({ key: folder.id, label: folder.name }))
    const unfiled = shown.some((item) => !folders.some((folder) => folder.id === item.folderId)) ? [{ key: '', label: 'Unfiled' }] : []
    return [...named, ...unfiled]
  }, [shown, folders])

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <SearchInput value={search} onChange={setSearch} placeholder="Search dashboards" className="w-full max-w-sm" />
        <SegmentedControl value={sort} options={DASH_SORTS} onChange={setSort} />
      </div>
      {shown.length === 0 && <p className="text-app-label text-muted-foreground">No dashboards match “{search}”.</p>}
      {groups.map((group) => {
        const items = shown.filter((item) => (group.key ? item.folderId === group.key : !folders.some((folder) => folder.id === item.folderId)))
        return (
          <section key={group.key || 'unfiled'} className="space-y-3">
            {groups.length > 1 && <SectionHeader title={group.label} description={`${items.length} ${items.length === 1 ? 'dashboard' : 'dashboards'}`} />}
            <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
              {items.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => onOpen(item.id)}
                  className="group flex flex-col rounded-xl border border-border bg-surface text-left transition-colors duration-fast hover:border-foreground/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                >
                  <span className="flex h-24 items-end gap-1.5 rounded-t-xl border-b border-border bg-muted/40 px-5 pb-4" aria-hidden>
                    {[38, 64, 46, 80, 56, 72, 90].map((height, index) => (
                      <span key={index} className="w-full rounded-t-sm bg-chart-1/25 transition-colors duration-fast group-hover:bg-chart-1/45" style={{ height: `${height}%` }} />
                    ))}
                  </span>
                  <span className="flex items-center gap-3 px-4 py-3">
                    <IconTile icon={LayoutDashboard} size="sm" />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-app-label font-semibold text-foreground">{item.name}</span>
                      <span className="block truncate text-app-caption text-muted-foreground">
                        {item.tileCount} {item.tileCount === 1 ? 'tile' : 'tiles'} · edited {relativeTime(item.updatedAt)}
                      </span>
                    </span>
                  </span>
                </button>
              ))}
            </div>
          </section>
        )
      })}
    </div>
  )
}

function VersionsList({ versions, pending, onRestore }: { versions: { id: string; createdAt: string }[]; pending: boolean; onRestore: (id: string) => void }) {
  const pg = usePagination(versions, { initialPageSize: 10 })
  return (
    <div className="space-y-2">
      <ul className="max-h-72 space-y-1 overflow-y-auto">
        {pg.pageItems.map((version) => (
          <li key={version.id} className="flex items-center justify-between gap-2 text-app-label">
            <span className="text-muted-foreground">{new Date(version.createdAt).toLocaleString()}</span>
            <ConfirmAction
              trigger={
                <Button variant="outline" size="sm" disabled={pending}>
                  Restore
                </Button>
              }
              title="Restore this version?"
              description="The dashboard is replaced with this version. You can restore another version afterwards."
              confirmLabel="Restore"
              onConfirm={() => onRestore(version.id)}
            />
          </li>
        ))}
      </ul>
      <PaginationBar page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} className="pt-1" />
    </div>
  )
}

function AddSqlTileDialog({ dashboardId, open, onOpenChange }: { dashboardId: string; open: boolean; onOpenChange: (open: boolean) => void }) {
  const [title, setTitle] = useState('')
  const [sql, setSql] = useState('')
  const add = useMutation({
    mutationFn: () => api.dashboardAddTile(dashboardId, title.trim() || 'Tile', sql.trim()),
    onSuccess: () => {
      setTitle('')
      setSql('')
      onOpenChange(false)
      void queryClient.invalidateQueries({ queryKey: queryKeys.dashboard(dashboardId) })
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent>
        <SheetHeader title="Add a tile from SQL" description="A read-only SELECT. The chart type is picked from the columns it returns." />
        <SheetBody>
        <form
          className="space-y-4"
          onSubmit={(event) => {
            event.preventDefault()
            if (sql.trim()) add.mutate()
          }}
        >
          <FormField label="Title" htmlFor="tile-title">
            <Input id="tile-title" value={title} onChange={(event) => setTitle(event.target.value)} placeholder="Revenue by month" />
          </FormField>
          <FormField label="SQL" htmlFor="tile-sql">
            <Textarea id="tile-sql" value={sql} onChange={(event) => setSql(event.target.value)} rows={5} className="font-mono text-app-caption" placeholder="SELECT ..." />
          </FormField>
          <Button type="submit" loading={add.isPending} disabled={!sql.trim()}>
            Add tile
          </Button>
        </form>
      
        </SheetBody>
      </SheetContent>
    </Sheet>
  )
}

export function DashboardsPage() {
  const dashboardId = useNavigation((state) => state.dashboardId)
  const openDashboard = useNavigation((state) => state.openDashboard)
  const list = useQuery({ queryKey: queryKeys.dashboards, queryFn: api.dashboards })
  const folders = useQuery({ queryKey: queryKeys.dashboardFolders, queryFn: api.dashboardFolders })
  const [prompt, setPrompt] = useState('')
  const tablesQuery = useQuery({ queryKey: queryKeys.tables, queryFn: api.tables })
  const tableNames = [...new Set((tablesQuery.data ?? []).map((table) => table.relation))].sort().slice(0, 12)
  const [folderName, setFolderName] = useState('')
  const [newOpen, setNewOpen] = useState(false)
  const [folderOpen, setFolderOpen] = useState(false)

  const generate = useMutation({
    mutationFn: () => api.dashboardGenerate(prompt),
    onMutate: () => useGeneration.getState().reset(),
    onSuccess: (live) => {
      setPrompt('')
      setNewOpen(false)
      void queryClient.invalidateQueries({ queryKey: queryKeys.dashboards })
      openDashboard(live.dashboard.id)
      if (live.warnings.length) toast.message(live.warnings.join(' '))
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const createFolder = useMutation({
    mutationFn: () => api.dashboardCreateFolder(folderName),
    onSuccess: () => {
      setFolderName('')
      setFolderOpen(false)
      void queryClient.invalidateQueries({ queryKey: queryKeys.dashboardFolders })
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  if (dashboardId) return <DashboardView id={dashboardId} />

  const dashboards = list.data ?? []
  const folderList = folders.data ?? []

  return (
    <PageContainer width="full">
      <PageHeader
        title="Dashboards"
        description="Charts built from your data. Each tile runs read-only SQL."
        actions={
          <Button onClick={() => setNewOpen(true)}>
            <Plus strokeWidth={iconStroke} /> New dashboard
          </Button>
        }
      />
      {list.isPending ? (
        <PageLoader />
      ) : dashboards.length === 0 ? (
        <EmptyState
          icon={LayoutDashboard}
          title="No dashboards yet"
          description="Describe what you want to see and LocalBoard builds the charts from your data."
          action={
            <Button onClick={() => setNewOpen(true)}>
              <Plus strokeWidth={iconStroke} /> New dashboard
            </Button>
          }
        />
      ) : (
        <DashboardGroups dashboards={dashboards} folders={folderList} onOpen={openDashboard} />
      )}
      <Sheet open={newOpen} onOpenChange={(open) => !generate.isPending && setNewOpen(open)}>
        <SheetContent width="lg">
          <SheetHeader title="New dashboard" description="Describe the charts you want in plain words. Anything that can't be built from your data is left out and listed." />
          <form
            className="flex min-h-0 flex-1 flex-col"
            onSubmit={(event) => {
              event.preventDefault()
              if (prompt.trim()) generate.mutate()
            }}
          >
            <SheetBody>
            <div className="grid grid-cols-2 gap-2 rounded-lg border border-border bg-muted/30 p-3 text-app-caption">
              {DASHBOARD_TIPS.map((tip) => (
                <div key={tip.label}>
                  <p className="font-medium text-foreground">{tip.label}</p>
                  <p className="text-muted-foreground">{tip.example}</p>
                </div>
              ))}
            </div>
            <Textarea
              autoFocus
              value={prompt}
              onChange={(event) => setPrompt(event.target.value)}
              rows={4}
              placeholder="Monthly revenue for the last 12 months, orders by region, top 10 customers by spend, and refunds as a share of orders"
              aria-label="Dashboard prompt"
            />
            {tableNames.length > 0 && (
              <div className="space-y-1.5">
                <p className="text-app-caption text-muted-foreground">Your tables (click to mention):</p>
                <div className="flex flex-wrap gap-1.5">
                  {tableNames.map((name) => (
                    <button
                      key={name}
                      type="button"
                      onClick={() => setPrompt((current) => `${current.trim()}${current.trim() ? ' ' : ''}${name}`)}
                      className="rounded-md border border-border bg-surface px-2 py-0.5 font-mono text-app-caption text-muted-foreground transition-colors duration-fast hover:text-foreground"
                    >
                      {name}
                    </button>
                  ))}
                </div>
              </div>
            )}
            <div className="space-y-1.5">
              <p className="text-app-caption text-muted-foreground">Or start from an example:</p>
              <div className="flex flex-wrap gap-1.5">
                {DASHBOARD_EXAMPLES.map((example) => (
                  <button
                    key={example}
                    type="button"
                    onClick={() => setPrompt(example)}
                    className="rounded-full border border-border bg-surface px-3 py-1 text-left text-app-caption text-muted-foreground transition-colors duration-fast hover:text-foreground"
                  >
                    {example}
                  </button>
                ))}
              </div>
            </div>
            <GenerationSteps running={generate.isPending} />
            </SheetBody>
            <SheetFooter>
              <Button type="button" variant="ghost" className="mr-auto" disabled={generate.isPending} onClick={() => setFolderOpen(true)}>
                New folder
              </Button>
              <Button type="button" variant="outline" disabled={generate.isPending} onClick={() => setNewOpen(false)}>
                Cancel
              </Button>
              <Button type="submit" loading={generate.isPending} disabled={!prompt.trim()}>
                Build dashboard
              </Button>
            </SheetFooter>
          </form>
        </SheetContent>
      </Sheet>
      <Sheet open={folderOpen} onOpenChange={setFolderOpen}>
        <SheetContent>
          <SheetHeader title="New folder" description="Folders group dashboards on this page." />
          <SheetBody>
          <form
            className="flex gap-2"
            onSubmit={(event) => {
              event.preventDefault()
              if (folderName.trim()) createFolder.mutate()
            }}
          >
            <Input autoFocus value={folderName} onChange={(event) => setFolderName(event.target.value)} placeholder="Folder name" aria-label="Folder name" />
            <Button type="submit" loading={createFolder.isPending} disabled={!folderName.trim()}>
              Add
            </Button>
          </form>
        
          </SheetBody>
        </SheetContent>
      </Sheet>
    </PageContainer>
  )
}

const CHART_KINDS: { value: ChartKind; label: string; icon: typeof BarChart3 }[] = [
  { value: 'bar', label: 'Bar', icon: BarChart3 },
  { value: 'line', label: 'Line', icon: LineChart },
  { value: 'area', label: 'Area', icon: AreaChart },
  { value: 'stacked', label: 'Stacked', icon: BarChartBig },
  { value: 'pie', label: 'Pie', icon: PieChart },
  { value: 'kpi', label: 'Number', icon: Hash },
  { value: 'table', label: 'Table', icon: Table2 },
  { value: 'funnel', label: 'Funnel', icon: Filter },
  { value: 'heatmap', label: 'Heatmap', icon: Grid3x3 },
]

const WIDTHS: { value: string; label: string }[] = [
  { value: '4', label: 'Third' },
  { value: '6', label: 'Half' },
  { value: '8', label: 'Two thirds' },
  { value: '12', label: 'Full' },
]

/** Fixed class names so Tailwind keeps them; tiles span 3 to 12 of 12 columns on wide screens. */
const SPAN: Record<number, string> = {
  3: 'lg:col-span-3',
  4: 'lg:col-span-4',
  5: 'lg:col-span-5',
  6: 'lg:col-span-6',
  7: 'lg:col-span-7',
  8: 'lg:col-span-8',
  9: 'lg:col-span-9',
  10: 'lg:col-span-10',
  11: 'lg:col-span-11',
  12: 'lg:col-span-12',
}

const spanOf = (tile: LiveTile) => {
  const w = Math.round(tile.layout.w || 6)
  // Tables and heatmaps need room; older dashboards stored them at half width.
  const fitted = ['table', 'heatmap'].includes(tile.chart.kind) ? Math.max(w, 8) : w
  return Math.min(12, Math.max(3, fitted))
}

/** Lays tiles out left to right in rows of 12 columns, in the given order. */
function packLayout(tiles: LiveTile[], widths: Record<string, number> = {}) {
  let x = 0
  let y = 0
  let rowHeight = 0
  return tiles.map((tile) => {
    const w = widths[tile.id] ?? spanOf(tile)
    const h = tile.layout.h || 5
    if (x + w > 12) {
      x = 0
      y += rowHeight
      rowHeight = 0
    }
    const placed = { id: tile.id, x, y, w, h }
    x += w
    rowHeight = Math.max(rowHeight, h)
    return placed
  })
}

function TileSheet({
  tile,
  onOpenChange,
  onWidth,
  onRemove,
  dashboardId,
}: {
  tile: LiveTile | null
  onOpenChange: (open: boolean) => void
  onWidth: (tile: LiveTile, width: number) => void
  onRemove: (tile: LiveTile) => void
  dashboardId: string
}) {
  const openBench = useNavigation((state) => state.openBench)
  const [title, setTitle] = useState('')
  const [sql, setSql] = useState('')
  const [kind, setKind] = useState<ChartKind>('bar')
  const [width, setWidth] = useState('6')
  const [removeOpen, setRemoveOpen] = useState(false)
  useEffect(() => {
    if (!tile) return
    setTitle(tile.title)
    setSql(tile.sql)
    setKind(tile.chart.kind)
    setWidth(String(spanOf(tile)))
  }, [tile])
  const isText = tile?.chart.kind === 'text'
  const queryChanged = Boolean(tile) && (title.trim() !== tile?.title || (!isText && (sql.trim() !== tile?.sql.trim() || kind !== tile?.chart.kind)))
  const widthChanged = Boolean(tile) && Number(width) !== (tile ? spanOf(tile) : 0)

  const saveTile = useMutation({
    mutationFn: async () => {
      if (!tile) return
      if (queryChanged) {
        if (isText) await api.dashboardUpdateTextTile(tile.id, title.trim(), tile.sql)
        else await api.dashboardUpdateTile(tile.id, title.trim(), sql.trim(), kind)
      }
      if (widthChanged) onWidth(tile, Number(width))
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.dashboard(dashboardId) })
      toast.success('Tile saved')
      onOpenChange(false)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  return (
    <Sheet open={tile !== null} onOpenChange={onOpenChange}>
      <SheetContent width="lg">
        <SheetHeader title="Tile settings" description={isText ? 'A note on this dashboard.' : 'Change what this tile shows and how wide it is.'} />
        <SheetBody>
          <FormField label="Name" htmlFor="tile-edit-title">
            <Input id="tile-edit-title" value={title} onChange={(event) => setTitle(event.target.value)} />
          </FormField>
          <FormField label="Width on wide screens">
            <SegmentedControl value={width} onChange={setWidth} options={WIDTHS} />
          </FormField>
          {!isText && (
            <>
              <div className="space-y-1.5">
                <Label>Chart</Label>
                <div className="grid grid-cols-3 gap-2">
                  {CHART_KINDS.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      aria-pressed={kind === option.value}
                      onClick={() => setKind(option.value)}
                      className={cn(
                        'flex items-center gap-2 rounded-lg border px-3 py-2 text-app-label transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40',
                        kind === option.value ? 'border-brand bg-brand/5 text-foreground ring-1 ring-brand/30' : 'border-border text-muted-foreground hover:text-foreground',
                      )}
                    >
                      <option.icon size={15} strokeWidth={iconStroke} />
                      {option.label}
                    </button>
                  ))}
                </div>
                <p className="text-app-caption text-muted-foreground">If the columns don't suit this chart, the closest one that fits is used.</p>
              </div>
              <FormField label="SQL" htmlFor="tile-edit-sql" hint="Read-only. It runs when you save, so mistakes show here before the tile changes.">
                <Textarea id="tile-edit-sql" value={sql} rows={10} onChange={(event) => setSql(event.target.value)} className="font-mono text-app-caption" />
              </FormField>
              <Button variant="outline" size="sm" onClick={() => openBench(sql)}>
                <Code2 strokeWidth={iconStroke} /> Open in Query Bench
              </Button>
              {tile?.error && (
                <Alert variant="destructive">
                  <AlertDescription className="whitespace-pre-wrap">{tile.error}</AlertDescription>
                </Alert>
              )}
            </>
          )}
        </SheetBody>
        <SheetFooter>
          <Button variant="ghost" className="mr-auto text-destructive hover:text-destructive" onClick={() => setRemoveOpen(true)}>
            <Trash2 strokeWidth={iconStroke} /> Remove tile
          </Button>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button loading={saveTile.isPending} disabled={!title.trim() || (!queryChanged && !widthChanged)} onClick={() => saveTile.mutate()}>
            Save
          </Button>
        </SheetFooter>
        <ConfirmDialog
          open={removeOpen}
          onOpenChange={setRemoveOpen}
          title="Remove this tile?"
          description="The tile is removed from this dashboard. Your data is not affected."
          confirmLabel="Remove tile"
          onConfirm={() => {
            if (tile) onRemove(tile)
            onOpenChange(false)
          }}
        />
      </SheetContent>
    </Sheet>
  )
}

function DashboardView({ id }: { id: string }) {
  const openDashboard = useNavigation((state) => state.openDashboard)
  const openBench = useNavigation((state) => state.openBench)
  const folders = useQuery({ queryKey: queryKeys.dashboardFolders, queryFn: api.dashboardFolders })
  const versions = useQuery({ queryKey: queryKeys.dashboardVersions(id), queryFn: () => api.dashboardVersions(id) })
  const [filterParams, setParams] = useState<Record<string, string>>({})
  const [cross, setCross] = useState<Record<string, string>>({})
  const params = useMemo(() => ({ ...filterParams, ...Object.fromEntries(Object.entries(cross).map(([column, value]) => [`__xf.${column}`, value])) }), [filterParams, cross])
  const [focus, setFocus] = useState<string | null>(null)
  const [editing, setEditing] = useState<string | null>(null)
  const [addSqlOpen, setAddSqlOpen] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [versionsOpen, setVersionsOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const live = useQuery({ queryKey: [...queryKeys.dashboard(id), params], queryFn: () => api.dashboard(id, params) })

  const dashboard = live.data?.dashboard
  const save = useMutation({
    mutationFn: (next: { name: string; folderId: string | null; refreshSecs: number | null }) =>
      api.dashboardUpdate({ id, name: next.name, folderId: next.folderId, filters: dashboard?.filters ?? [], refreshSecs: next.refreshSecs }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.dashboards })
      void queryClient.invalidateQueries({ queryKey: queryKeys.dashboard(id) })
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const duplicate = useMutation({
    mutationFn: () => api.dashboardDuplicate(id),
    onSuccess: (copy) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.dashboards })
      openDashboard(copy.id)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const removeDashboard = useMutation({
    mutationFn: () => api.dashboardDelete(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.dashboards })
      openDashboard(null)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const removeTile = useMutation({
    mutationFn: (tileId: string) => api.dashboardDeleteTile(tileId),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.dashboard(id) }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const saveLayout = useMutation({
    mutationFn: api.dashboardSaveLayout,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.dashboard(id) }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const restore = useMutation({
    mutationFn: (versionId: string) => api.dashboardRestore(id, versionId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.dashboard(id) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.dashboardVersions(id) })
      toast.success('Restored')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const refresh = useMutation({
    mutationFn: () => api.dashboardRefresh(id, params),
    onSuccess: (next) => queryClient.setQueryData([...queryKeys.dashboard(id), params], next),
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  const addNote = useMutation({
    mutationFn: () => api.dashboardAddTextTile(id, 'Note', '## Note\nDouble-click to edit this **markdown** text.'),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.dashboard(id) }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const selectDimension = (column: string | null | undefined, value: string) => {
    if (!column) return
    setCross((current) => ({ ...current, [column]: value }))
  }

  const allTiles = live.data?.tiles ?? []
  // KPI numbers read best as a strip above the charts; the rest share one grid.
  const kpiTiles = allTiles.filter((tile) => tile.chart.kind === 'kpi' && tile.result)
  const tiles = allTiles.filter((tile) => !kpiTiles.includes(tile)).sort((a, b) => a.layout.y - b.layout.y || a.layout.x - b.layout.x)
  const focused = tiles.find((tile) => tile.id === focus) ?? null
  const editingTile = allTiles.find((tile) => tile.id === editing) ?? null
  const move = (tile: LiveTile, offset: number) => {
    const index = tiles.indexOf(tile)
    const target = index + offset
    if (index < 0 || target < 0 || target >= tiles.length) return
    const next = [...tiles]
    ;[next[index], next[target]] = [next[target], next[index]]
    saveLayout.mutate(packLayout(next))
  }
  const resize = (tile: LiveTile, width: number) => {
    if (tile.chart.kind === 'kpi') return
    saveLayout.mutate(packLayout(tiles, { [tile.id]: width }))
  }
  const pickSegment = (tile: LiveTile, name: string) => {
    const key = tile.chart.x ?? 'segment'
    if (dashboard?.filters.some((filter) => filter.name === key)) setParams((current) => ({ ...current, [key]: name }))
    selectDimension(tile.chart.x, name)
  }

  if (live.isPending || !dashboard) return <PageLoader />

  return (
    <div className="space-y-4 pb-6">
      <DetailHeader
        onBack={() => openDashboard(null)}
        title={
          <Input
            defaultValue={dashboard.name}
            aria-label="Dashboard name"
            className="max-w-xs"
            onBlur={(event) => save.mutate({ name: event.target.value, folderId: dashboard.folderId, refreshSecs: dashboard.refreshSecs })}
          />
        }
        actions={
          <>
            {dashboard.refreshSecs ? <StatusBadge variant="success">Auto-refresh {dashboard.refreshSecs}s</StatusBadge> : null}
            <span className="text-app-caption text-muted-foreground">Updated {relativeTime(live.dataUpdatedAt ? new Date(live.dataUpdatedAt).toISOString() : null)}</span>
            <Button variant="outline" loading={refresh.isPending} onClick={() => refresh.mutate()}>
              {!refresh.isPending && <RefreshCw strokeWidth={iconStroke} />} Refresh
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline">
                  <Plus strokeWidth={iconStroke} /> Add tile
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem disabled={addNote.isPending} onSelect={() => addNote.mutate()}>
                  Note
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setAddSqlOpen(true)}>From SQL…</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
        menu={
          <>
            <DropdownMenuItem onSelect={() => setSettingsOpen(true)}>Folder and refresh…</DropdownMenuItem>
            <DropdownMenuItem disabled={duplicate.isPending} onSelect={() => duplicate.mutate()}>
              Duplicate
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => setVersionsOpen(true)}>Versions…</DropdownMenuItem>
            <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setDeleteOpen(true)}>
              <Trash2 size={14} strokeWidth={iconStroke} /> Delete dashboard
            </DropdownMenuItem>
          </>
        }
      />
      <ConfirmDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title="Delete this dashboard?"
        description="Its tiles and saved versions are removed. Your data is not affected."
        confirmLabel="Delete dashboard"
        onConfirm={() => removeDashboard.mutate()}
      />
      <Sheet open={settingsOpen} onOpenChange={setSettingsOpen}>
        <SheetContent>
          <SheetHeader title="Folder and refresh" description="Where this dashboard is filed and how often it refreshes." />
          <SheetBody>
          <FormField label="Folder" htmlFor="dash-folder">
            <Select
              value={dashboard.folderId ?? NO_FOLDER}
              onValueChange={(value) => save.mutate({ name: dashboard.name, folderId: value === NO_FOLDER ? null : value, refreshSecs: dashboard.refreshSecs })}
            >
              <SelectTrigger id="dash-folder">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_FOLDER}>No folder</SelectItem>
                {(folders.data ?? []).map((folder) => (
                  <SelectItem key={folder.id} value={folder.id}>
                    {folder.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>
          <FormField label="Refresh interval" htmlFor="dash-refresh">
            <Select
              value={String(dashboard.refreshSecs ?? 0)}
              onValueChange={(value) => save.mutate({ name: dashboard.name, folderId: dashboard.folderId, refreshSecs: Number(value) || null })}
            >
              <SelectTrigger id="dash-refresh">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="0">Manual refresh</SelectItem>
                <SelectItem value="30">30 seconds</SelectItem>
                <SelectItem value="60">1 minute</SelectItem>
                <SelectItem value="300">5 minutes</SelectItem>
              </SelectContent>
            </Select>
          </FormField>
        
          </SheetBody>
        </SheetContent>
      </Sheet>
      <Sheet open={versionsOpen} onOpenChange={setVersionsOpen}>
        <SheetContent>
          <SheetHeader title="Versions" description="Restore an earlier version of this dashboard." />
          <SheetBody>
          {(versions.data ?? []).length === 0 ? (
            <EmptyState title="No versions yet" />
          ) : (
            <VersionsList versions={versions.data ?? []} pending={restore.isPending} onRestore={(id) => restore.mutate(id)} />
          )}
        
          </SheetBody>
        </SheetContent>
      </Sheet>
      <div className="space-y-4 px-6">
      {Object.keys(cross).length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-app-caption text-muted-foreground">Cross-filters</span>
          {Object.entries(cross).map(([column, value]) => (
            <Button
              key={column}
              type="button"
              variant="outline"
              size="sm"
              aria-label={`Remove filter ${column}`}
              onClick={() => setCross((current) => Object.fromEntries(Object.entries(current).filter(([key]) => key !== column)))}
            >
              {column} = {value}
              <X className="size-3" strokeWidth={iconStroke} />
            </Button>
          ))}
        </div>
      )}
      {live.data?.warnings.map((warning) => (
        <Alert key={warning} variant="destructive">
          <AlertDescription>{warning}</AlertDescription>
        </Alert>
      ))}
      {dashboard.filters.length > 0 && (
        <div className="sticky top-[3.55rem] z-[5] -mx-6 flex flex-wrap items-end gap-3 border-b border-border bg-background/85 px-6 py-2.5 backdrop-blur">
          {dashboard.filters.map((filter) => (
            <label key={filter.name} className="flex flex-col gap-1">
              <span className="text-app-caption font-medium text-muted-foreground">{filter.label || filter.name}</span>
              <Input
                type={filter.kind === 'date' ? 'date' : 'text'}
                value={filterParams[filter.name] ?? ''}
                aria-label={filter.label || filter.name}
                className="h-8 max-w-[12rem]"
                onChange={(event) => setParams((current) => ({ ...current, [filter.name]: event.target.value }))}
              />
            </label>
          ))}
          {Object.values(filterParams).some(Boolean) && (
            <Button variant="ghost" size="sm" onClick={() => setParams({})}>
              Clear filters
            </Button>
          )}
        </div>
      )}
      {kpiTiles.length > 0 && (
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {kpiTiles.map((tile) => (
            <Card key={tile.id} className="relative p-4">
              <p className="pr-7 text-app-caption font-medium text-muted-foreground">{tile.title}</p>
              {tile.result && <ResultChart spec={tile.chart} result={tile.result} />}
              <div className="absolute right-1.5 top-1.5">
                <RowMenu label={`Actions for ${tile.title}`} className="h-7 w-7">
                  <DropdownMenuItem onSelect={() => setEditing(tile.id)}>Edit tile…</DropdownMenuItem>
                  <DropdownMenuItem onSelect={() => openBench(tile.sql)}>Open SQL in Query Bench</DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => removeTile.mutate(tile.id)}>
                    Remove tile
                  </DropdownMenuItem>
                </RowMenu>
              </div>
            </Card>
          ))}
        </div>
      )}
      {tiles.length === 0 && kpiTiles.length === 0 && (
        <EmptyState
          icon={LayoutDashboard}
          title="No tiles to show"
          description="Every tile was dropped, or none is built yet. Add one from SQL, or build the dashboard again."
          action={<Button onClick={() => setAddSqlOpen(true)}>Add a tile from SQL</Button>}
        />
      )}
      {tiles.length > 0 && (
        <div className="grid gap-4 lg:grid-cols-12">
          {tiles.map((tile, index) => (
            <Card key={tile.id} className={cn('group flex min-w-0 flex-col', SPAN[spanOf(tile)])}>
              <div className="flex items-center justify-between gap-2 px-4 py-2.5">
                <h3 className="truncate text-app-label font-semibold text-foreground">{tile.title}</h3>
                <div className="flex shrink-0 items-center gap-1">
                  {tile.chart.kind !== 'text' && tile.result && (
                    <Hint text="Open large">
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 opacity-0 transition-opacity duration-fast focus-visible:opacity-100 group-hover:opacity-100"
                        aria-label={`Open ${tile.title}`}
                        onClick={() => setFocus(tile.id)}
                      >
                        <Maximize2 strokeWidth={iconStroke} />
                      </Button>
                    </Hint>
                  )}
                  <Hint text="Tile settings">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 opacity-0 transition-opacity duration-fast focus-visible:opacity-100 group-hover:opacity-100"
                      aria-label={`Settings for ${tile.title}`}
                      onClick={() => setEditing(tile.id)}
                    >
                      <SlidersHorizontal strokeWidth={iconStroke} />
                    </Button>
                  </Hint>
                  <RowMenu label={`Actions for ${tile.title}`} className="h-7 w-7">
                    <DropdownMenuItem onSelect={() => setEditing(tile.id)}>Edit tile…</DropdownMenuItem>
                    {tile.chart.kind !== 'text' && <DropdownMenuItem onSelect={() => openBench(tile.sql)}>Open SQL in Query Bench</DropdownMenuItem>}
                    <DropdownMenuSeparator />
                    <DropdownMenuItem disabled={index === 0 || saveLayout.isPending} onSelect={() => move(tile, -1)}>
                      Move earlier
                    </DropdownMenuItem>
                    <DropdownMenuItem disabled={index === tiles.length - 1 || saveLayout.isPending} onSelect={() => move(tile, 1)}>
                      Move later
                    </DropdownMenuItem>
                    <DropdownMenuSub>
                      <DropdownMenuSubTrigger>Width</DropdownMenuSubTrigger>
                      <DropdownMenuSubContent>
                        {WIDTHS.map((option) => (
                          <DropdownMenuItem key={option.value} onSelect={() => resize(tile, Number(option.value))}>
                            {spanOf(tile) === Number(option.value) && <Check size={14} strokeWidth={iconStroke} />}
                            {option.label}
                          </DropdownMenuItem>
                        ))}
                      </DropdownMenuSubContent>
                    </DropdownMenuSub>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => removeTile.mutate(tile.id)}>
                      Remove tile
                    </DropdownMenuItem>
                  </RowMenu>
                </div>
              </div>
              <div className="flex-1 px-4 pb-4">
                {tile.error && (
                  <Alert variant="destructive">
                    <AlertDescription className="space-y-2">
                      <p className="whitespace-pre-wrap">{tile.error}</p>
                      <Button variant="outline" size="sm" onClick={() => openBench(tile.sql)}>
                        Open in Query Bench
                      </Button>
                    </AlertDescription>
                  </Alert>
                )}
                {tile.chart.kind === 'text' && (
                  <TextTile
                    key={tile.sql}
                    id={tile.id}
                    title={tile.title}
                    markdown={tile.sql}
                    onSaved={() => void queryClient.invalidateQueries({ queryKey: queryKeys.dashboard(id) })}
                  />
                )}
                {tile.result && tile.chart.kind !== 'text' && (
                  <ResultChart spec={tile.chart} result={tile.result} onSelect={(name) => pickSegment(tile, name)} onSelectCell={selectDimension} />
                )}
                {tile.insight && <p className="mt-2 text-app-caption text-muted-foreground">{tile.insight}</p>}
              </div>
            </Card>
          ))}
        </div>
      )}
      <Sheet open={focused !== null} onOpenChange={(open) => !open && setFocus(null)}>
        <SheetContent width="xl">
          <SheetHeader title={focused?.title ?? ''} description={focused?.insight ? focused.insight : undefined} />
          <SheetBody>
          {focused?.result && (
            <ResultChart spec={focused.chart} result={focused.result} tall onSelect={(name) => pickSegment(focused, name)} onSelectCell={selectDimension} />
          )}
        
          </SheetBody>
        </SheetContent>
      </Sheet>
      <AddSqlTileDialog dashboardId={id} open={addSqlOpen} onOpenChange={setAddSqlOpen} />
      <TileSheet
        tile={editingTile}
        dashboardId={id}
        onOpenChange={(open) => !open && setEditing(null)}
        onWidth={resize}
        onRemove={(tile) => removeTile.mutate(tile.id)}
      />
      </div>
    </div>
  )
}
