import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { save } from '@tauri-apps/plugin-dialog'
import { AlignLeft, ArrowDown, ArrowUp, BarChart3, Download, FileText, Globe, Link2, Plus, Presentation, Table2, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { Alert, AlertDescription } from '../../components/ui/alert'
import { Badge } from '../../components/ui/badge'
import { Button } from '../../components/ui/button'
import { DropdownMenuItem, DropdownMenuSeparator } from '../../components/ui/dropdown-menu'
import { Input } from '../../components/ui/input'
import { PaginationBar } from '../../components/ui/pagination'
import { FormSheet } from '../../components/ui/sheet'
import { Textarea } from '../../components/ui/textarea'
import { Hint } from '../../components/ui/tooltip'
import { ConfirmDialog, CopyField, DetailHeader, EmptyState, FormField, PageContainer, PageHeader, PageLoader, SearchInput } from '../../components/layout/ui'
import { api, errorMessage, type ArtifactSummary, type OutlineSlide } from '../../lib/api'
import { relativeTime } from '../../lib/time'
import { IconTile, PaneItem, SaveBar, SplitView } from '../../components/layout/patterns'
import { Tabs, TabsList, TabsTrigger } from '../../components/ui/tabs'
import { cn } from '../../lib/utils'
import { usePagination } from '../../lib/usePagination'
import { iconStroke } from '../../lib/icon'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { GenerationSteps } from '../../components/analyst/StepsPanel'
import { useGeneration } from '../../stores/generation'
import { useNavigation, type ArtifactTab } from '../../stores/navigation'

const TABS: { value: ArtifactTab; label: string }[] = [
  { value: 'sites', label: 'Sites' },
  { value: 'reports', label: 'Reports' },
  { value: 'decks', label: 'Decks' },
]

const TYPE_ICON: Record<ArtifactTab, typeof Globe> = { sites: Globe, reports: FileText, decks: Presentation }

const EXPLAINER: Record<ArtifactTab, string> = {
  sites: 'A small website with an overview and sections. It stays live at a local link while LocalBoard runs, and exports as HTML.',
  reports: 'A written report with charts, tables and the SQL behind them. Exports as a PDF.',
  decks: 'An editable slide outline with charts. Exports as PowerPoint.',
}

export function DatasitesPage() {
  const tab = useNavigation((state) => state.artifactTab)
  const setTab = useNavigation((state) => state.setArtifactTab)
  const datasiteId = useNavigation((state) => state.datasiteId)
  const reportId = useNavigation((state) => state.reportId)
  const deckId = useNavigation((state) => state.deckId)
  const { data: status } = useQuery({ queryKey: queryKeys.status, queryFn: api.appStatus })
  const sites = useQuery({ queryKey: queryKeys.datasites, queryFn: api.datasites })
  const reports = useQuery({ queryKey: queryKeys.reports, queryFn: api.reports })
  const decks = useQuery({ queryKey: queryKeys.decks, queryFn: api.decks })
  const counts: Partial<Record<ArtifactTab, number>> = { sites: sites.data?.length, reports: reports.data?.length, decks: decks.data?.length }

  if (tab === 'sites' && datasiteId) return <DatasiteView id={datasiteId} />
  if (tab === 'reports' && reportId) return <ReportView id={reportId} />
  if (tab === 'decks' && deckId) return <DeckView id={deckId} />

  return (
    <PageContainer width="full">
      <PageHeader
        title="Publish"
        description="Turn your data into a shareable site, a PDF report or a slide deck."
        actions={
          status?.localServer ? (
            <Hint text={`Live URL origin ${status.localServer.url}`}>
              <Badge variant="success">
                <span className="h-1.5 w-1.5 rounded-full bg-success" /> Sites are live
              </Badge>
            </Hint>
          ) : status?.localServerError ? (
            <Hint text={status.localServerError}>
              <Badge variant="destructive">Sites are offline</Badge>
            </Hint>
          ) : undefined
        }
      />
      <Tabs value={tab} onValueChange={(value) => setTab(value as ArtifactTab)}>
          <TabsList>
            {TABS.map((item) => {
              const Icon = TYPE_ICON[item.value]
              return (
                <TabsTrigger key={item.value} value={item.value} title={EXPLAINER[item.value]} className="group gap-2">
                  <Icon size={15} strokeWidth={iconStroke} />
                  {item.label}
                  <span className="rounded-full bg-muted px-1.5 text-app-caption tabular-nums text-muted-foreground group-data-[state=active]:bg-brand/15 group-data-[state=active]:text-brand">
                    {counts[item.value] ?? 0}
                  </span>
                </TabsTrigger>
              )
            })}
          </TabsList>
        </Tabs>
      {tab === 'sites' && <SiteList />}
      {tab === 'reports' && <ReportList />}
      {tab === 'decks' && <DeckList />}
    </PageContainer>
  )
}

/** List of built artifacts with a "New" dialog, shared by sites, reports and decks. */
function ArtifactTabPanel({
  prompt,
  onPrompt,
  placeholder,
  promptLabel,
  buildLabel,
  building,
  onBuild,
  loading,
  items,
  unit,
  icon: Icon,
  noun,
  onOpen,
}: {
  prompt: string
  onPrompt: (value: string) => void
  placeholder: string
  promptLabel: string
  buildLabel: string
  building: boolean
  onBuild: () => void
  loading: boolean
  items: ArtifactSummary[]
  unit: string
  icon: typeof Globe
  noun: string
  onOpen: (id: string) => void
}) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const term = search.trim().toLowerCase()
  const shown = (term ? items.filter((item) => item.name.toLowerCase().includes(term)) : items).slice().sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
  const pg = usePagination(shown, { initialPageSize: 12, resetKey: `${unit}|${term}` })
  const newButton = (
    <Button onClick={() => setOpen(true)}>
      <Plus strokeWidth={iconStroke} /> New {noun}
    </Button>
  )
  return (
    <div className="space-y-5">
      {loading ? (
        <PageLoader />
      ) : items.length === 0 ? (
        <EmptyState icon={Icon} title={`No ${noun}s yet`} description="Describe what you want and it is built from your data." action={newButton} />
      ) : (
        <>
          <div className="flex flex-wrap items-center justify-between gap-3">
            <SearchInput value={search} onChange={setSearch} placeholder={`Search ${noun}s`} className="w-full max-w-sm" />
            {newButton}
          </div>
          {shown.length === 0 && <p className="text-app-label text-muted-foreground">Nothing matches “{search}”.</p>}
          <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
            {pg.pageItems.map((item) => (
              <button
                key={item.id}
                type="button"
                onClick={() => onOpen(item.id)}
                className="group flex flex-col rounded-xl border border-border bg-surface text-left transition-colors duration-fast hover:border-foreground/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
              >
                <span className="flex h-28 flex-col justify-center gap-2 rounded-t-xl border-b border-border bg-muted/40 px-6" aria-hidden>
                  <span className="h-2.5 w-2/3 rounded-full bg-foreground/15" />
                  <span className="h-2 w-full rounded-full bg-foreground/10" />
                  <span className="h-2 w-5/6 rounded-full bg-foreground/10" />
                  <span className="mt-1 flex items-end gap-1">
                    {[40, 70, 55, 85].map((height, index) => (
                      <span key={index} className="w-4 rounded-t-sm bg-chart-2/30 group-hover:bg-chart-2/50" style={{ height: `${height * 0.24}px` }} />
                    ))}
                  </span>
                </span>
                <span className="flex items-center gap-3 px-4 py-3">
                  <IconTile icon={Icon} size="sm" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-app-label font-semibold text-foreground">{item.name}</span>
                    <span className="block truncate text-app-caption text-muted-foreground">
                      {item.itemCount} {item.itemCount === 1 ? unit.replace(/s$/, '') : unit} · edited {relativeTime(item.updatedAt)}
                    </span>
                  </span>
                </span>
              </button>
            ))}
          </div>
          <PaginationBar page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} className="pt-1" />
        </>
      )}
      <FormSheet
        open={open}
        onOpenChange={(next) => !building && setOpen(next)}
        title={`New ${noun}`}
        description="Describe what it should cover. Charts that cannot be built are dropped and named."
        submitLabel={buildLabel}
        onSubmit={onBuild}
        submitting={building}
        canSubmit={Boolean(prompt.trim())}
        width="lg"
      >
        <Textarea autoFocus value={prompt} onChange={(event) => onPrompt(event.target.value)} rows={5} placeholder={placeholder} aria-label={promptLabel} />
        <GenerationSteps running={building} />
      </FormSheet>
    </div>
  )
}

function SiteList() {
  const open = useNavigation((state) => state.openDatasite)
  const list = useQuery({ queryKey: queryKeys.datasites, queryFn: api.datasites })
  const [prompt, setPrompt] = useState('')
  const generate = useMutation({
    mutationFn: () => api.datasiteGenerate(prompt),
    onMutate: () => useGeneration.getState().reset(),
    onSuccess: (live) => {
      setPrompt('')
      void queryClient.invalidateQueries({ queryKey: queryKeys.datasites })
      open(live.datasite.id)
      if (live.warnings.length) toast.message(live.warnings.join(' '))
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <ArtifactTabPanel
      prompt={prompt}
      onPrompt={setPrompt}
      placeholder="Overview of orders, refunds and the West region"
      promptLabel="Datasite prompt"
      buildLabel="Build datasite"
      building={generate.isPending}
      onBuild={() => generate.mutate()}
      loading={list.isLoading}
      items={list.data ?? []}
      unit="pages"
      icon={Globe}
      noun="site"
      onOpen={open}
    />
  )
}

function ReportList() {
  const open = useNavigation((state) => state.openReport)
  const list = useQuery({ queryKey: queryKeys.reports, queryFn: api.reports })
  const [prompt, setPrompt] = useState('')
  const generate = useMutation({
    mutationFn: () => api.reportGenerate(prompt),
    onMutate: () => useGeneration.getState().reset(),
    onSuccess: (live) => {
      setPrompt('')
      void queryClient.invalidateQueries({ queryKey: queryKeys.reports })
      open(live.report.id)
      if (live.warnings.length) toast.message(live.warnings.join(' '))
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <ArtifactTabPanel
      prompt={prompt}
      onPrompt={setPrompt}
      placeholder="A written report on last month"
      promptLabel="Report prompt"
      buildLabel="Build PDF report"
      building={generate.isPending}
      onBuild={() => generate.mutate()}
      loading={list.isLoading}
      items={list.data ?? []}
      unit="blocks"
      icon={FileText}
      noun="report"
      onOpen={open}
    />
  )
}

function DeckList() {
  const open = useNavigation((state) => state.openDeck)
  const list = useQuery({ queryKey: queryKeys.decks, queryFn: api.decks })
  const [prompt, setPrompt] = useState('')
  const generate = useMutation({
    mutationFn: () => api.deckGenerate(prompt),
    onMutate: () => useGeneration.getState().reset(),
    onSuccess: (live) => {
      setPrompt('')
      void queryClient.invalidateQueries({ queryKey: queryKeys.decks })
      open(live.deck.id)
      if (live.warnings.length) toast.message(live.warnings.join(' '))
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <ArtifactTabPanel
      prompt={prompt}
      onPrompt={setPrompt}
      placeholder="A short deck of the main findings"
      promptLabel="Deck prompt"
      buildLabel="Build presentation"
      building={generate.isPending}
      onBuild={() => generate.mutate()}
      loading={list.isLoading}
      items={list.data ?? []}
      unit="slides"
      icon={Presentation}
      noun="deck"
      onOpen={open}
    />
  )
}

function LoadError({ error }: { error: unknown }) {
  return (
    <div className="px-8 py-10">
      <Alert variant="destructive">
        <AlertDescription>{errorMessage(error)}</AlertDescription>
      </Alert>
    </div>
  )
}

function DatasiteView({ id }: { id: string }) {
  const back = useNavigation((state) => state.openDatasite)
  const live = useQuery({ queryKey: queryKeys.datasite(id), queryFn: () => api.datasite(id) })
  const remove = useMutation({
    mutationFn: () => api.datasiteDelete(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.datasites })
      back(null)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const exp = useMutation({
    mutationFn: async (mode: 'static' | 'live') => {
      const path = await save({ defaultPath: `${live.data?.datasite.name ?? 'datasite'}.html`, filters: [{ name: 'HTML', extensions: ['html'] }] })
      if (!path) return false
      await api.datasiteExport(id, mode, path)
      return true
    },
    onSuccess: (saved) => saved && toast.success('Exported'),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [pageId, setPageId] = useState<string | null>(null)
  if (live.error) return <LoadError error={live.error} />
  if (!live.data) return <PageLoader />
  const pages = [...live.data.datasite.pages].sort((a, b) => a.sort - b.sort)
  const page = pages.find((item) => item.id === pageId) ?? pages[0]
  const pageUrl = !page || page.slug === 'overview' ? live.data.url : `${live.data.url}/${page.slug}`
  return (
    <div className="flex h-[calc(100%-28px)] flex-col">
      <DetailHeader
        title={live.data.datasite.name}
        onBack={() => back(null)}
        actions={
          <>
            <div className="w-72">
              <CopyField value={live.data.url} label="Copy link" />
            </div>
          </>
        }
        menu={
          <>
            <DropdownMenuItem disabled={exp.isPending} onSelect={() => exp.mutate('static')}>
              <Link2 size={14} strokeWidth={iconStroke} /> Export snapshot (works offline)
            </DropdownMenuItem>
            <DropdownMenuItem disabled={exp.isPending} onSelect={() => exp.mutate('live')}>
              <Link2 size={14} strokeWidth={iconStroke} /> Export live page (needs LocalBoard running)
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setDeleteOpen(true)}>
              <Trash2 size={14} strokeWidth={iconStroke} /> Delete
            </DropdownMenuItem>
          </>
        }
      />
      <SplitView
        listWidth={240}
        list={
          <>
            <div className="px-4 pb-2 pt-4 text-app-caption font-medium text-muted-foreground">Pages · {pages.length}</div>
            <div className="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-2 pb-4">
              {pages.map((item) => (
                <PaneItem
                  key={item.id}
                  selected={item.id === page?.id}
                  onClick={() => setPageId(item.id)}
                  leading={<IconTile icon={FileText} size="sm" tone="muted" />}
                  title={item.title}
                  meta={`${item.charts.length} ${item.charts.length === 1 ? 'chart' : 'charts'}`}
                />
              ))}
            </div>
            {live.data.datasite.prompt && (
              <div className="border-t border-border px-4 py-3">
                <p className="text-app-caption font-medium text-muted-foreground">Built from</p>
                <p className="mt-1 line-clamp-4 text-app-caption text-foreground">{live.data.datasite.prompt}</p>
              </div>
            )}
          </>
        }
        detail={
          <div className="flex h-full flex-col bg-muted/30 p-4">
            <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-border bg-card shadow-md">
              <div className="flex items-center gap-2 border-b border-border px-3 py-2">
                <span className="flex gap-1" aria-hidden>
                  <span className="size-2.5 rounded-full bg-border" />
                  <span className="size-2.5 rounded-full bg-border" />
                  <span className="size-2.5 rounded-full bg-border" />
                </span>
                <span className="min-w-0 flex-1 truncate rounded-md bg-muted px-2 py-0.5 font-mono text-app-caption text-muted-foreground">{pageUrl}</span>
              </div>
              <iframe key={pageUrl} title={page?.title ?? live.data.datasite.name} src={pageUrl} className="min-h-0 w-full flex-1 bg-white" sandbox="allow-scripts" />
            </div>
          </div>
        }
      />
      <ConfirmDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title="Delete this datasite?"
        description="The pages and their saved results are removed. Your data is not affected."
        onConfirm={() => remove.mutate()}
      />
    </div>
  )
}

function ReportView({ id }: { id: string }) {
  const back = useNavigation((state) => state.openReport)
  const live = useQuery({ queryKey: queryKeys.report(id), queryFn: () => api.report(id) })
  const { data: status } = useQuery({ queryKey: queryKeys.status, queryFn: api.appStatus })
  const remove = useMutation({
    mutationFn: () => api.reportDelete(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.reports })
      back(null)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const exp = useMutation({
    mutationFn: async () => {
      const path = await save({ defaultPath: `${live.data?.report.name ?? 'report'}.pdf`, filters: [{ name: 'PDF', extensions: ['pdf'] }] })
      if (!path) return false
      await api.reportExportPdf(id, path)
      return true
    },
    onSuccess: (saved) => saved && toast.success('PDF saved'),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const [deleteOpen, setDeleteOpen] = useState(false)
  if (live.error) return <LoadError error={live.error} />
  if (!live.data) return <PageLoader />
  const preview = status?.localServer ? `${status.localServer.url}/r/${id}` : null
  const blocks = [...live.data.report.blocks].sort((a, b) => a.sort - b.sort)
  return (
    <div className="flex h-[calc(100%-28px)] flex-col">
      <DetailHeader
        title={live.data.report.name}
        onBack={() => back(null)}
        actions={
          <Button onClick={() => exp.mutate()} loading={exp.isPending}>
            <Download strokeWidth={iconStroke} /> Export PDF
          </Button>
        }
        menu={
          <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setDeleteOpen(true)}>
            <Trash2 size={14} strokeWidth={iconStroke} /> Delete
          </DropdownMenuItem>
        }
      />
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1 bg-muted/30 p-4">
          {preview ? (
            <iframe title={live.data.report.name} src={preview} className="h-full w-full rounded-xl border border-border bg-white shadow-md" sandbox="allow-scripts" />
          ) : (
            <div className="mx-auto h-full max-w-3xl space-y-6 overflow-y-auto rounded-xl border border-border bg-card px-10 py-8 shadow-md">
              {blocks.map((block) => (
                <section key={block.id} id={`block-${block.id}`} className="space-y-1">
                  <h2 className="font-serif text-app-title">{block.title}</h2>
                  <p className="whitespace-pre-wrap text-app-label text-muted-foreground">{block.body}</p>
                </section>
              ))}
            </div>
          )}
        </div>
        <aside className="hidden w-64 shrink-0 flex-col border-l border-border bg-sidebar/40 lg:flex">
          <div className="px-4 pb-2 pt-4 text-app-caption font-medium text-muted-foreground">Contents · {blocks.length}</div>
          <ol className="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-2 pb-4">
            {blocks.map((block, index) => {
              const Icon = block.chart ? BarChart3 : block.sql ? Table2 : AlignLeft
              const body = (
                <>
                  <span className="w-4 text-right text-app-caption tabular-nums text-muted-foreground">{index + 1}</span>
                  <Icon size={14} strokeWidth={iconStroke} className="shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1 truncate text-app-label text-foreground">{block.title || 'Untitled'}</span>
                </>
              )
              return (
                <li key={block.id}>
                  {preview ? (
                    <div className="flex items-center gap-2 rounded-lg px-2 py-1.5">{body}</div>
                  ) : (
                    <button
                      type="button"
                      onClick={() => document.getElementById(`block-${block.id}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' })}
                      className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left transition-colors duration-fast hover:bg-accent/50"
                    >
                      {body}
                    </button>
                  )}
                </li>
              )
            })}
          </ol>
          {live.data.report.prompt && (
            <div className="border-t border-border px-4 py-3">
              <p className="text-app-caption font-medium text-muted-foreground">Built from</p>
              <p className="mt-1 line-clamp-4 text-app-caption text-foreground">{live.data.report.prompt}</p>
            </div>
          )}
        </aside>
      </div>
      <ConfirmDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title="Delete this report?"
        description="The report and its saved results are removed. Your data is not affected."
        onConfirm={() => remove.mutate()}
      />
    </div>
  )
}

function DeckView({ id }: { id: string }) {
  const back = useNavigation((state) => state.openDeck)
  const live = useQuery({ queryKey: queryKeys.deck(id), queryFn: () => api.deck(id) })
  const [outline, setOutline] = useState<OutlineSlide[] | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const slides = outline ?? live.data?.deck.outline ?? []
  const selectedIndex = Math.max(0, slides.findIndex((slide) => slide.id === selectedId))
  const selected = slides[selectedIndex]
  const withData = new Set((live.data?.deck.slides ?? []).filter((slide) => slide.sql).map((slide) => slide.id))
  const remove = useMutation({
    mutationFn: () => api.deckDelete(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.decks })
      back(null)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const saveOutline = useMutation({
    mutationFn: () => api.deckUpdateOutline(id, slides),
    onSuccess: () => {
      setOutline(null)
      void queryClient.invalidateQueries({ queryKey: queryKeys.deck(id) })
      void queryClient.invalidateQueries({ queryKey: queryKeys.decks })
      toast.success('Slides saved')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const exp = useMutation({
    mutationFn: async () => {
      if (outline) {
        await api.deckUpdateOutline(id, outline)
        setOutline(null)
        void queryClient.invalidateQueries({ queryKey: queryKeys.deck(id) })
      }
      const path = await save({ defaultPath: `${live.data?.deck.name ?? 'deck'}.pptx`, filters: [{ name: 'PowerPoint', extensions: ['pptx'] }] })
      if (!path) return false
      await api.deckExportPptx(id, path)
      return true
    },
    onSuccess: (saved) => saved && toast.success('PPTX saved'),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const [deleteOpen, setDeleteOpen] = useState(false)
  if (live.error) return <LoadError error={live.error} />
  if (!live.data) return <PageLoader />

  const edit = (patch: Partial<OutlineSlide>) => setOutline(slides.map((item, index) => (index === selectedIndex ? { ...item, ...patch } : item)))
  const move = (offset: number) => {
    const target = selectedIndex + offset
    if (target < 0 || target >= slides.length) return
    const next = [...slides]
    ;[next[selectedIndex], next[target]] = [next[target], next[selectedIndex]]
    setOutline(next)
  }
  const add = () => {
    const slide = { id: crypto.randomUUID(), title: 'New slide', body: '' }
    const next = [...slides]
    next.splice(selectedIndex + 1, 0, slide)
    setOutline(next)
    setSelectedId(slide.id)
  }
  const drop = () => {
    if (slides.length <= 1) return
    const next = slides.filter((_, index) => index !== selectedIndex)
    setOutline(next)
    setSelectedId(next[Math.min(selectedIndex, next.length - 1)].id)
  }

  return (
    <div className="flex h-[calc(100%-28px)] flex-col">
      <DetailHeader
        title={
          <div className="flex min-w-0 items-center gap-2">
            <h1 className="truncate font-serif text-app-title text-foreground">{live.data.deck.name}</h1>
            <Badge variant={outline ? 'warning' : 'secondary'}>{outline ? 'Unsaved' : `${slides.length} slides`}</Badge>
          </div>
        }
        onBack={() => back(null)}
        actions={
          <>
            <Button variant="outline" onClick={() => saveOutline.mutate()} disabled={!outline} loading={saveOutline.isPending}>
              Save
            </Button>
            <Button onClick={() => exp.mutate()} loading={exp.isPending}>
              <Download strokeWidth={iconStroke} /> Export PPTX
            </Button>
          </>
        }
        menu={
          <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setDeleteOpen(true)}>
            <Trash2 size={14} strokeWidth={iconStroke} /> Delete
          </DropdownMenuItem>
        }
      />
      <SplitView
        listWidth={232}
        list={
          <>
            <div className="flex items-center justify-between px-4 pb-2 pt-4">
              <span className="text-app-caption font-medium text-muted-foreground">Slides</span>
              <Hint text="Add a slide after this one">
                <Button variant="ghost" size="icon" aria-label="Add slide" onClick={add}>
                  <Plus strokeWidth={iconStroke} />
                </Button>
              </Hint>
            </div>
            <ol className="min-h-0 flex-1 space-y-2 overflow-y-auto px-3 pb-4">
              {slides.map((slide, index) => (
                <li key={slide.id} className="flex gap-2">
                  <span className="w-4 pt-1 text-right text-app-caption tabular-nums text-muted-foreground">{index + 1}</span>
                  <button
                    type="button"
                    onClick={() => setSelectedId(slide.id)}
                    aria-current={index === selectedIndex ? 'true' : undefined}
                    className={cn(
                      'flex aspect-video min-w-0 flex-1 flex-col gap-1 overflow-hidden rounded-md border bg-card p-2 text-left shadow-sm transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40',
                      index === selectedIndex ? 'border-primary ring-1 ring-primary' : 'border-border hover:border-foreground/30',
                    )}
                  >
                    <span className="line-clamp-2 font-serif text-[11px] leading-tight text-foreground">{slide.title || 'Untitled'}</span>
                    {withData.has(slide.id) ? (
                      <span className="mt-auto flex h-5 items-end gap-0.5" aria-hidden>
                        {[40, 70, 55, 90, 65].map((height, bar) => (
                          <span key={bar} className="w-1.5 rounded-sm bg-chart-1/60" style={{ height: `${height}%` }} />
                        ))}
                      </span>
                    ) : (
                      <span className="line-clamp-2 text-[9px] leading-tight text-muted-foreground">{slide.body}</span>
                    )}
                  </button>
                </li>
              ))}
            </ol>
          </>
        }
        detail={
          selected && (
            <div className="mx-auto max-w-4xl space-y-5 px-8 py-6">
              <div className="flex items-center gap-2">
                <span className="text-app-caption text-muted-foreground">
                  Slide {selectedIndex + 1} of {slides.length}
                </span>
                {withData.has(selected.id) && <Badge variant="secondary">Chart from your data</Badge>}
                <div className="ml-auto flex items-center gap-1">
                  <Hint text="Move up">
                    <Button variant="ghost" size="icon" aria-label="Move up" disabled={selectedIndex === 0} onClick={() => move(-1)}>
                      <ArrowUp strokeWidth={iconStroke} />
                    </Button>
                  </Hint>
                  <Hint text="Move down">
                    <Button variant="ghost" size="icon" aria-label="Move down" disabled={selectedIndex === slides.length - 1} onClick={() => move(1)}>
                      <ArrowDown strokeWidth={iconStroke} />
                    </Button>
                  </Hint>
                  <Hint text={slides.length <= 1 ? 'A deck needs at least one slide' : 'Remove slide'}>
                    <Button variant="ghost" size="icon" aria-label="Remove slide" disabled={slides.length <= 1} onClick={drop}>
                      <Trash2 strokeWidth={iconStroke} />
                    </Button>
                  </Hint>
                </div>
              </div>
              <div className="flex aspect-video flex-col gap-4 overflow-hidden rounded-xl border border-border bg-card p-10 shadow-md">
                <h2 className="font-serif text-2xl leading-tight text-foreground">{selected.title || 'Untitled'}</h2>
                {withData.has(selected.id) ? (
                  <div className="flex min-h-0 flex-1 items-end gap-3 border-b border-l border-border px-4 pb-0" aria-label="Chart placeholder">
                    {[35, 60, 45, 80, 55, 70].map((height, bar) => (
                      <span key={bar} className="flex-1 rounded-t-sm bg-chart-1/50" style={{ height: `${height}%` }} />
                    ))}
                  </div>
                ) : null}
                {selected.body && (
                  <p className={cn('whitespace-pre-wrap text-app-label text-muted-foreground', withData.has(selected.id) && 'line-clamp-2')}>{selected.body}</p>
                )}
              </div>
              <p className="text-app-caption text-muted-foreground">Preview only. Charts are drawn from fresh data when you export.</p>
              <div className="grid gap-4">
                <FormField label="Title" htmlFor="slide-title">
                  <Input id="slide-title" value={selected.title} onChange={(event) => edit({ title: event.target.value })} />
                </FormField>
                <FormField label="Notes on the slide" htmlFor="slide-body">
                  <Textarea id="slide-body" value={selected.body} rows={5} onChange={(event) => edit({ body: event.target.value })} />
                </FormField>
              </div>
              <SaveBar dirty={!!outline} saving={saveOutline.isPending} onSave={() => saveOutline.mutate()} onDiscard={() => setOutline(null)} />
            </div>
          )
        }
      />
      <ConfirmDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title="Delete this presentation?"
        description="The slides are removed. Your data is not affected."
        onConfirm={() => remove.mutate()}
      />
    </div>
  )
}
