import { useEffect, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import {
  Activity,
  ArrowDown,
  ArrowUp,
  Bell,
  CalendarClock,
  Database,
  FileDown,
  FileSearch,
  FileText,
  Gauge,
  GitBranch,
  GitCompare,
  Hand,
  LayoutDashboard,
  Mail,
  MessageSquare,
  Play,
  Plus,
  Repeat,
  Sparkles,
  Timer,
  Trash2,
  Webhook,
  Wrench,
  X,
  type LucideIcon,
} from 'lucide-react'
import { toast } from 'sonner'
import { Badge } from '../../components/ui/badge'
import { Button } from '../../components/ui/button'
import { Checkbox } from '../../components/ui/checkbox'
import { Sheet, SheetBody, SheetContent, SheetHeader } from '../../components/ui/sheet'
import { DropdownMenuItem } from '../../components/ui/dropdown-menu'
import { Hint } from '../../components/ui/tooltip'
import { Input } from '../../components/ui/input'
import { Label } from '../../components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../../components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../../components/ui/table'
import { Textarea } from '../../components/ui/textarea'
import { ConfirmDialog, CopyField, DetailHeader, EmptyState, FormField, MoreMenu, PageContainer, PageHeader, SectionHeader, ViewToggle } from '../../components/layout/ui'
import { useViewMode } from '../../lib/useViewMode'
import { GenerationSteps } from '../../components/analyst/StepsPanel'
import { Banner, ChoiceGrid, IconTile, Meter, Panel, SaveBar, StatRow, Timeline } from '../../components/layout/patterns'
import { Switch } from '../../components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../components/ui/tabs'
import { relativeTime } from '../../lib/time'
import { api, errorMessage, type Automation, type AutomationRun, type StepKind, type TriggerKind } from '../../lib/api'
import { PaginationBar } from '../../components/ui/pagination'
import { usePagination } from '../../lib/usePagination'
import { iconStroke } from '../../lib/icon'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { useNavigation } from '../../stores/navigation'
import { useGeneration } from '../../stores/generation'

const NONE = '__none__'

const TRIGGERS: [string, string][] = [
  ['interval', 'On an interval'],
  ['cron', 'Cron schedule'],
  ['file_change', 'When a file changes'],
  ['manual', 'Manual only'],
  ['webhook', 'Local webhook (127.0.0.1)'],
]
const INTERVALS: [string, string][] = [
  ['300', 'Every 5 minutes'],
  ['900', 'Every 15 minutes'],
  ['3600', 'Every hour'],
  ['86400', 'Every day'],
]
const STEP_KINDS: [string, string][] = [
  ['sql', 'Run SQL'],
  ['ask', 'Ask analyst'],
  ['dashboard', 'Refresh dashboard'],
  ['compare', 'Compare to previous run'],
  ['threshold', 'Threshold check'],
  ['anomaly', 'Anomaly check'],
  ['report', 'Generate report or deck'],
  ['email', 'Email (SMTP, leaves device)'],
  ['mcp_tool', 'Call MCP tool'],
  ['webhook', 'Send webhook (POST JSON)'],
  ['write_file', 'Write CSV or JSON file'],
  ['notify', 'Desktop notification'],
  ['branch', 'Continue only if…'],
]

/** A labelled single-choice field. Empty string maps to a sentinel because Radix items cannot be empty. */
function Pick({
  value,
  onChange,
  options,
  label,
  placeholder,
  className,
}: {
  value: string
  onChange: (value: string) => void
  options: [string, string][]
  label: string
  placeholder?: string
  className?: string
}) {
  return (
    <Select value={value === '' ? NONE : value} onValueChange={(next) => onChange(next === NONE ? '' : next)}>
      <SelectTrigger className={className} aria-label={label}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {options.map(([optionValue, optionLabel]) => (
          <SelectItem key={optionValue || NONE} value={optionValue === '' ? NONE : optionValue}>
            {optionLabel}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

function statusVariant(status: string | null | undefined) {
  if (!status) return 'secondary' as const
  if (status === 'ok' || status === 'success') return 'success' as const
  if (status === 'error' || status === 'failed') return 'destructive' as const
  return 'secondary' as const
}

type DraftStep = { id?: string; key: string; kind: StepKind; config: Record<string, unknown> }

// `key` stays stable while steps are added, removed or reordered, so typed secrets follow their step.
const emptyStep = (kind: StepKind = 'sql'): DraftStep => ({ key: crypto.randomUUID(), kind, config: {} })

const STEP_ICON: Record<string, LucideIcon> = {
  sql: Database,
  ask: MessageSquare,
  dashboard: LayoutDashboard,
  compare: GitCompare,
  threshold: Gauge,
  anomaly: Activity,
  report: FileText,
  email: Mail,
  mcp_tool: Wrench,
  webhook: Webhook,
  write_file: FileDown,
  notify: Bell,
  branch: GitBranch,
}

const STEP_GROUPS: { label: string; kinds: StepKind[] }[] = [
  { label: 'Get data', kinds: ['sql', 'ask', 'dashboard'] as StepKind[] },
  { label: 'Check', kinds: ['compare', 'threshold', 'anomaly', 'branch'] as StepKind[] },
  { label: 'Produce', kinds: ['report', 'write_file'] as StepKind[] },
  { label: 'Deliver', kinds: ['notify', 'email', 'webhook', 'mcp_tool'] as StepKind[] },
]

const STEP_HELP: Record<string, string> = {
  sql: 'Read rows with a guarded SELECT.',
  ask: 'Ask the analyst a question.',
  dashboard: 'Refresh a dashboard.',
  compare: 'Compare with the previous run.',
  threshold: 'Alert when a value crosses a line.',
  anomaly: 'Flag unusual periods.',
  branch: 'Stop unless a condition holds.',
  report: 'Build a PDF report or deck.',
  write_file: 'Save the result as CSV or JSON.',
  notify: 'Show a desktop notification.',
  email: 'Send an email (leaves this device).',
  webhook: 'POST the result as JSON.',
  mcp_tool: 'Call a tool on an MCP server.',
}

const TRIGGER_ICON: Record<string, LucideIcon> = { interval: Repeat, cron: CalendarClock, file_change: FileSearch, manual: Hand, webhook: Webhook }

function stepLabel(kind: string) {
  return STEP_KINDS.find(([value]) => value === kind)?.[1] ?? kind
}

/** The trigger in words, for lists and headers. */
function triggerWords(kind: string, trigger: Record<string, unknown>) {
  if (kind === 'interval') {
    const secs = Number(trigger.intervalSecs ?? 0)
    return INTERVALS.find(([value]) => Number(value) === secs)?.[1] ?? (secs ? `Every ${Math.round(secs / 60)} minutes` : 'On an interval')
  }
  if (kind === 'cron') return cronWords(String(trigger.cron ?? ''))
  if (kind === 'file_change') {
    const path = String(trigger.path ?? '')
    return path ? `When ${path.split('/').pop() || path} changes` : 'When a file changes'
  }
  if (kind === 'webhook') return 'When the webhook is called'
  return 'Manual only'
}

const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday']

/** Reads the common five-field patterns aloud; anything else is shown as written. */
function cronWords(cron: string) {
  const parts = cron.trim().split(/\s+/)
  if (parts.length !== 5) return cron ? `Cron ${cron}` : 'Cron schedule'
  const [minute, hour, day, month, weekday] = parts
  const time = /^\d+$/.test(minute) && /^\d+$/.test(hour) ? `${hour.padStart(2, '0')}:${minute.padStart(2, '0')}` : null
  if (minute === '0' && hour === '*' && day === '*' && month === '*' && weekday === '*') return 'Every hour, on the hour'
  if (time && day === '*' && month === '*' && weekday === '*') return `Every day at ${time}`
  if (time && day === '*' && month === '*' && /^\d$/.test(weekday)) return `Every ${DAYS[Number(weekday)] ?? weekday} at ${time}`
  if (time && day === '*' && month === '*' && weekday === '1-5') return `Weekdays at ${time}`
  if (time && /^\d+$/.test(day) && month === '*' && weekday === '*') return `Day ${day} of each month at ${time}`
  return `Cron ${cron}`
}

const IDEAS = [
  'Every morning, tell me if yesterday’s refunds were above 50',
  'Every Monday at 9:00, build a PDF report of last week’s orders',
  'When orders.csv changes, refresh the sales dashboard',
]

function triggerFrom(job?: Automation): { kind: TriggerKind; intervalSecs: number; cron: string; path: string } {
  return {
    kind: job?.triggerKind ?? 'interval',
    intervalSecs: Number(job?.trigger.intervalSecs ?? 3600),
    cron: String(job?.trigger.cron ?? '0 * * * *'),
    path: String(job?.trigger.path ?? ''),
  }
}

export function AutomationsPage() {
  const list = useQuery({ queryKey: queryKeys.automations, queryFn: api.automations })
  const dashboards = useQuery({ queryKey: queryKeys.dashboards, queryFn: api.dashboards })
  const [selected, setSelected] = useState('')
  const [name, setName] = useState('')
  const [triggerKind, setTriggerKind] = useState<TriggerKind>('interval')
  const [intervalSecs, setIntervalSecs] = useState(3600)
  const [cron, setCron] = useState('0 * * * *')
  const [path, setPath] = useState('')
  const [onChangeOnly, setOnChangeOnly] = useState(true)
  const [steps, setSteps] = useState<DraftStep[]>([emptyStep()])
  const [idea, setIdea] = useState('')
  const [draftOpen, setDraftOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [token, setToken] = useState<string | null>(null)
  const [stepSecrets, setStepSecrets] = useState<Record<string, string>>({})
  const [creating, setCreating] = useState(false)
  const [pickerOpen, setPickerOpen] = useState(false)
  const [tab, setTab] = useState('setup')
  const mcpServers = useQuery({ queryKey: queryKeys.mcpServers, queryFn: api.mcpServers })
  const runs = useQuery({
    queryKey: queryKeys.automationRuns(selected),
    queryFn: () => api.automationRuns(selected),
    enabled: Boolean(selected),
  })
  const job = list.data?.find((item) => item.id === selected)
  const jobPg = usePagination(list.data ?? [], { initialPageSize: 12 })
  const [view, setView] = useViewMode('automations')
  const runPg = usePagination(runs.data ?? [], { initialPageSize: 10, resetKey: selected })
  const requested = useNavigation((state) => state.automationId)
  useEffect(() => {
    const item = requested ? list.data?.find((entry) => entry.id === requested) : undefined
    if (item) {
      load(item)
      useNavigation.setState({ automationId: null })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requested, list.data])

  const trigger =
    triggerKind === 'interval'
      ? { intervalSecs }
      : triggerKind === 'cron'
        ? { cron }
        : triggerKind === 'file_change'
          ? { path }
          : {}

  const save = useMutation({
    mutationFn: async () => {
      const prepared = steps.map((step) => {
        const id = step.id ?? step.key
        const typed = stepSecrets[step.key]
        const config = step.kind === 'webhook' && typed ? { ...step.config, secret: true } : step.config
        return { id, kind: step.kind, config, typed }
      })
      for (const step of prepared) {
        if (step.kind === 'webhook' && step.typed) await api.automationSetStepSecret(step.id, step.typed)
      }
      return api.automationSave({
        id: selected || undefined,
        name,
        triggerKind,
        trigger,
        steps: prepared.map(({ id, kind, config }) => ({ id, kind, config })),
        enabled: job ? job.enabled : true,
        onChangeOnly,
      })
    },
    onSuccess: (saved) => {
      setSelected(saved.id)
      setCreating(false)
      setSteps((current) => current.map((step) => ({ ...step, id: step.id ?? step.key })))
      setStepSecrets({})
      if (saved.webhookToken) setToken(saved.webhookToken)
      void queryClient.invalidateQueries({ queryKey: queryKeys.automations })
      toast.success('Saved')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const run = useMutation({
    mutationFn: () => api.automationRun(selected),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.automations })
      void queryClient.invalidateQueries({ queryKey: queryKeys.automationRuns(selected) })
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const remove = useMutation({
    mutationFn: () => api.automationDelete(selected),
    onSuccess: () => {
      setSelected('')
      setCreating(false)
      void queryClient.invalidateQueries({ queryKey: queryKeys.automations })
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const toggle = useMutation({
    mutationFn: (item: Automation) =>
      api.automationSave({
        id: item.id,
        name: item.name,
        triggerKind: item.triggerKind,
        trigger: item.trigger,
        steps: item.steps.map((step) => ({ id: step.id, kind: step.kind, config: step.config })),
        enabled: !item.enabled,
        onChangeOnly: item.onChangeOnly,
      }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.automations }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const confirm = useMutation({
    mutationFn: () => api.automationConfirm(selected),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.automations })
      toast.success('Confirmed. You can enable it now.')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const propose = useMutation({
    mutationFn: () => api.automationPropose(idea),
    onMutate: () => useGeneration.getState().reset(),
    onSuccess: (saved) => {
      load(saved)
      setDraftOpen(false)
      setIdea('')
      void queryClient.invalidateQueries({ queryKey: queryKeys.automations })
      toast.success('Review the draft before enabling it')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const rotate = useMutation({
    mutationFn: () => api.automationRotateWebhook(selected),
    onSuccess: (created) => {
      setToken(created.token)
      void queryClient.invalidateQueries({ queryKey: queryKeys.automations })
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  const load = (item: Automation) => {
    const next = triggerFrom(item)
    setSelected(item.id)
    setName(item.name)
    setTriggerKind(next.kind)
    setIntervalSecs(next.intervalSecs)
    setCron(next.cron)
    setPath(next.path)
    setOnChangeOnly(item.onChangeOnly)
    setSteps(item.steps.map((step) => ({ id: step.id, key: step.id, kind: step.kind, config: { ...step.config } })))
    setStepSecrets({})
    setToken(null)
    setCreating(false)
  }

  const reset = () => {
    setSelected('')
    setName('')
    setTriggerKind('interval')
    setIntervalSecs(3600)
    setCron('0 * * * *')
    setPath('')
    setOnChangeOnly(true)
    setSteps([emptyStep()])
    setStepSecrets({})
    setToken(null)
    setCreating(true)
  }

  const updateStep = (index: number, patch: Partial<DraftStep>) => {
    setSteps((current) => current.map((step, i) => (i === index ? { ...step, ...patch } : step)))
  }

  const setConfig = (index: number, key: string, value: unknown) => {
    setSteps((current) =>
      current.map((step, i) => (i === index ? { ...step, config: { ...step.config, [key]: value } } : step)),
    )
  }

  const jobs = list.data ?? []
  const editing = Boolean(selected) || creating
  const savedSnapshot = job
    ? JSON.stringify({ n: job.name, k: job.triggerKind, t: triggerFrom(job), o: job.onChangeOnly, s: job.steps.map((step) => [step.kind, step.config]) })
    : ''
  const currentSnapshot = JSON.stringify({ n: name, k: triggerKind, t: { kind: triggerKind, intervalSecs, cron, path }, o: onChangeOnly, s: steps.map((step) => [step.kind, step.config]) })
  const dirty = creating || (Boolean(job) && savedSnapshot !== currentSnapshot) || Object.values(stepSecrets).some(Boolean)

  /** Switching jobs would drop unsaved edits, so ask first. */
  const guard = (next: () => void) => {
    if (dirty && editing && !window.confirm('Discard unsaved changes to this job?')) return
    next()
  }

  const runRows = runs.data ?? []
  const okRuns = runRows.filter((entry) => statusVariant(entry.status) === 'success').length
  const moveStep = (index: number, delta: number) =>
    setSteps((current) => {
      const next = [...current]
      const target = index + delta
      if (target < 0 || target >= next.length) return current
      ;[next[index], next[target]] = [next[target], next[index]]
      return next
    })

  const back = () =>
    guard(() => {
      setSelected('')
      setCreating(false)
    })
  const statusDot = (item: Automation) => (
    <span
      className={`size-2 shrink-0 rounded-full ${item.needsReview ? 'bg-warning' : !item.enabled ? 'bg-muted-foreground/40' : statusVariant(item.lastStatus) === 'destructive' ? 'bg-destructive' : 'bg-success'}`}
      aria-hidden
    />
  )
  const statusBadge = (item: Automation) =>
    item.needsReview ? <Badge variant="warning">Needs review</Badge> : !item.enabled ? <Badge variant="secondary">Paused</Badge> : <Badge variant="success">On</Badge>

  const jobList = (
    <section className="space-y-4">
      <div className="flex items-center justify-end">
        <ViewToggle value={view} onChange={setView} />
      </div>
      {view === 'cards' ? (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {jobPg.pageItems.map((item) => {
            const Icon = TRIGGER_ICON[item.triggerKind] ?? Timer
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => load(item)}
                className="flex flex-col gap-3 rounded-xl border border-border bg-surface p-4 text-left transition-colors duration-fast hover:border-foreground/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
              >
                <span className="flex items-start gap-3">
                  <IconTile icon={Icon} tone={item.enabled && !item.needsReview ? 'brand' : 'muted'} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-app-label font-semibold text-foreground">{item.name}</span>
                    <span className="block truncate text-app-caption text-muted-foreground">{triggerWords(item.triggerKind, item.trigger)}</span>
                  </span>
                  {statusBadge(item)}
                </span>
                <span className="mt-auto flex items-center gap-2 text-app-caption text-muted-foreground">
                  {statusDot(item)}
                  {item.lastRunAt ? `Last ran ${relativeTime(item.lastRunAt)}` : 'Never run'} · {item.steps.length} {item.steps.length === 1 ? 'step' : 'steps'}
                </span>
              </button>
            )
          })}
        </div>
      ) : (
        <div className="overflow-hidden rounded-xl border border-border bg-surface">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Job</TableHead>
                <TableHead>Runs when</TableHead>
                <TableHead>Steps</TableHead>
                <TableHead>Last run</TableHead>
                <TableHead>Status</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {jobPg.pageItems.map((item) => (
                <TableRow key={item.id} className="cursor-pointer" onClick={() => load(item)}>
                  <TableCell>
                    <span className="flex items-center gap-2.5">
                      {statusDot(item)}
                      <span className="font-medium text-foreground">{item.name}</span>
                    </span>
                  </TableCell>
                  <TableCell className="text-muted-foreground">{triggerWords(item.triggerKind, item.trigger)}</TableCell>
                  <TableCell className="tabular-nums text-muted-foreground">{item.steps.length}</TableCell>
                  <TableCell className="text-muted-foreground">{item.lastRunAt ? relativeTime(item.lastRunAt) : 'Never'}</TableCell>
                  <TableCell>{statusBadge(item)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
      <PaginationBar alwaysShow page={jobPg.page} pageSize={jobPg.pageSize} total={jobPg.total} onPageChange={jobPg.setPage} onPageSizeChange={jobPg.setPageSize} />
    </section>
  )

  const welcome = (
    <div className="mx-auto flex max-w-2xl flex-col items-center px-8 py-16 text-center">
      <IconTile icon={Timer} size="lg" />
      <h1 className="mt-4 font-serif text-app-page text-foreground">Put your checks on autopilot</h1>
      <p className="mt-2 text-app-label text-muted-foreground">
        Jobs run on a schedule, when a file changes, or on demand, and keep running after you close the window.
      </p>
      <div className="mt-8 w-full rounded-2xl border border-input bg-surface p-3 text-left shadow-sm">
        <Textarea
          value={idea}
          onChange={(event) => setIdea(event.target.value)}
          rows={3}
          className="border-0 bg-transparent shadow-none focus-visible:ring-0"
          placeholder="Describe a job, and the analyst drafts it for you to review"
          aria-label="Describe the job"
        />
        <div className="flex items-center justify-between gap-2 pt-1">
          <Button variant="ghost" size="sm" onClick={reset}>
            Start from scratch
          </Button>
          <Button size="sm" loading={propose.isPending} disabled={!idea.trim()} onClick={() => propose.mutate()}>
            {!propose.isPending && <Sparkles strokeWidth={iconStroke} />} Draft it
          </Button>
        </div>
        {propose.isPending && (
          <div className="mt-2 border-t border-border px-1 pt-3">
            <GenerationSteps running />
          </div>
        )}
      </div>
      <div className="mt-4 flex flex-wrap justify-center gap-2">
        {IDEAS.map((text) => (
          <button
            key={text}
            type="button"
            onClick={() => setIdea(text)}
            className="rounded-full border border-border bg-surface px-3 py-1 text-app-caption text-muted-foreground transition-colors duration-fast hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
          >
            {text}
          </button>
        ))}
      </div>
    </div>
  )

  const detail = (
    <div className="mx-auto max-w-4xl space-y-6 px-8 py-8">
      <header className="flex flex-wrap items-start gap-4">
        <IconTile icon={TRIGGER_ICON[triggerKind] ?? Timer} size="lg" />
        <div className="min-w-0 flex-1">
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="Name this job"
            aria-label="Job name"
            className="-ml-3 w-[calc(100%+0.75rem)] rounded-lg border border-transparent bg-transparent px-3 py-1 font-serif text-app-page text-foreground transition-colors duration-fast placeholder:text-muted-foreground/60 hover:border-border focus:border-ring/60 focus:outline-none"
          />
          <p className="mt-1 flex flex-wrap items-center gap-2 text-app-label text-muted-foreground">
            {triggerWords(triggerKind, trigger)}
            {job?.needsReview && <Badge variant="warning">Needs review</Badge>}
            {creating && <Badge variant="secondary">Not saved yet</Badge>}
          </p>
        </div>
        {job && (
          <div className="flex shrink-0 items-center gap-3">
            <label className="flex items-center gap-2 text-app-label text-foreground">
              <Switch checked={job.enabled} disabled={job.needsReview || toggle.isPending} onCheckedChange={() => toggle.mutate(job)} aria-label="Enabled" />
              {job.enabled ? 'On' : 'Paused'}
            </label>
            <Button variant="outline" loading={run.isPending} disabled={job.needsReview} onClick={() => run.mutate()}>
              {!run.isPending && <Play strokeWidth={iconStroke} />} Run now
            </Button>
            <MoreMenu>
              <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setDeleteOpen(true)}>
                <Trash2 size={14} strokeWidth={iconStroke} /> Delete job
              </DropdownMenuItem>
            </MoreMenu>
          </div>
        )}
      </header>

      {job?.needsReview && (
        <Banner
          icon={Sparkles}
          tone="warning"
          title="Review this draft"
          description="Written by the local model. Check every step, then confirm before it can run."
          actions={
            <Button size="sm" loading={confirm.isPending} onClick={() => confirm.mutate()}>
              Confirm
            </Button>
          }
        />
      )}

      {job && (
        <StatRow
          stats={[
            { label: 'Runs', value: runRows.length, icon: Repeat, onClick: () => setTab('history') },
            {
              label: 'Success rate',
              value: runRows.length ? `${Math.round((okRuns / runRows.length) * 100)}%` : '—',
              hint: runRows.length ? <Meter value={(okRuns / runRows.length) * 100} tone={okRuns === runRows.length ? 'success' : 'warning'} className="mt-1" /> : 'No runs yet',
              icon: Activity,
            },
            { label: 'Last run', value: job.lastRunAt ? relativeTime(job.lastRunAt) : 'Never', hint: job.lastSummary ?? undefined, icon: CalendarClock },
          ]}
        />
      )}

      <Tabs value={job ? tab : 'setup'} onValueChange={setTab}>
        <TabsList variant="underline">
          <TabsTrigger variant="underline" value="setup">
            Setup
          </TabsTrigger>
          <TabsTrigger variant="underline" value="history" disabled={!job}>
            History {runRows.length > 0 && <span className="ml-1.5 rounded-full bg-muted px-1.5 text-app-caption tabular-nums text-muted-foreground">{runRows.length}</span>}
          </TabsTrigger>
        </TabsList>
        <TabsContent value="setup" className="space-y-6">
          <Panel title="When it runs">
            <div className="space-y-4">
              <ChoiceGrid
                columns={3}
                value={triggerKind}
                onSelect={(value) => setTriggerKind(value as TriggerKind)}
                options={TRIGGERS.map(([value, label]) => ({ value, title: label, icon: TRIGGER_ICON[value] ?? Timer }))}
              />
              {triggerKind === 'interval' && (
                <FormField label="How often">
                  <Pick value={String(intervalSecs)} onChange={(value) => setIntervalSecs(Number(value))} options={INTERVALS} label="Interval" className="max-w-xs" />
                </FormField>
              )}
              {triggerKind === 'cron' && (
                <FormField label="Cron expression" htmlFor="cron" hint={`minute hour day month weekday · reads as: ${cronWords(cron)}`}>
                  <Input id="cron" value={cron} onChange={(event) => setCron(event.target.value)} className="max-w-xs font-mono" />
                </FormField>
              )}
              {triggerKind === 'file_change' && (
                <FormField label="Watch path" htmlFor="watch-path">
                  <Input id="watch-path" value={path} onChange={(event) => setPath(event.target.value)} placeholder="/path/to/file-or-folder" />
                </FormField>
              )}
              {triggerKind === 'webhook' && (
                <div className="space-y-3">
                  <FormField label="Webhook URL" hint="POST with Authorization: Bearer <token>. Only this computer can call it.">
                    <CopyField value={job?.webhookUrl ?? 'Save the job to get its URL'} label="Copy URL" />
                  </FormField>
                  {token && (
                    <FormField label="Token (shown once)">
                      <CopyField value={token} label="Copy token" />
                    </FormField>
                  )}
                  {selected && (
                    <Button type="button" variant="outline" size="sm" onClick={() => rotate.mutate()}>
                      New token
                    </Button>
                  )}
                </div>
              )}
              <label className="flex items-center gap-2 text-app-label text-foreground">
                <Switch checked={onChangeOnly} onCheckedChange={setOnChangeOnly} />
                Notify only when the result changes
              </label>
            </div>
          </Panel>

          <section className="space-y-3">
            <SectionHeader
              title="Steps"
              description="Each step gets the result of the one before it."
              actions={
                <Button size="sm" onClick={() => setPickerOpen(true)}>
                  <Plus strokeWidth={iconStroke} /> Add step
                </Button>
              }
            />
            {steps.length === 0 ? (
              <EmptyState icon={Plus} title="No steps yet" description="Start with Run SQL to fetch the data to check." action={<Button onClick={() => setPickerOpen(true)}>Add step</Button>} />
            ) : (
              <Timeline
                items={steps.map((step, index) => ({
                  id: step.key,
                  icon: STEP_ICON[step.kind] ?? Timer,
                  status: 'idle' as const,
                  title: (
                    <span className="flex items-center gap-2">
                      <span className="text-app-caption tabular-nums text-muted-foreground">{index + 1}</span>
                      {stepLabel(step.kind)}
                    </span>
                  ),
                  meta: STEP_HELP[step.kind],
                  actions: (
                    <>
                      <Hint text="Move up">
                        <Button type="button" variant="ghost" size="icon" className="h-7 w-7" disabled={index === 0} aria-label={`Move step ${index + 1} up`} onClick={() => moveStep(index, -1)}>
                          <ArrowUp strokeWidth={iconStroke} />
                        </Button>
                      </Hint>
                      <Hint text="Move down">
                        <Button type="button" variant="ghost" size="icon" className="h-7 w-7" disabled={index === steps.length - 1} aria-label={`Move step ${index + 1} down`} onClick={() => moveStep(index, 1)}>
                          <ArrowDown strokeWidth={iconStroke} />
                        </Button>
                      </Hint>
                      <Hint text="Remove step">
                        <Button type="button" variant="ghost" size="icon" className="h-7 w-7" aria-label={`Remove step ${index + 1}`} onClick={() => setSteps((current) => current.filter((item) => item.key !== step.key))}>
                          <X strokeWidth={iconStroke} />
                        </Button>
                      </Hint>
                    </>
                  ),
                  children: <div className="space-y-3">
                    {step.kind === 'sql' && (
                      <FormField label="SQL">
                        <Textarea value={String(step.config.sql ?? '')} onChange={(event) => setConfig(index, 'sql', event.target.value)} rows={3} className="font-mono" placeholder="SELECT …" />
                      </FormField>
                    )}
                    {step.kind === 'ask' && (
                      <FormField label="Question">
                        <Textarea value={String(step.config.question ?? '')} onChange={(event) => setConfig(index, 'question', event.target.value)} rows={2} />
                      </FormField>
                    )}
                    {step.kind === 'dashboard' && (
                      <FormField label="Dashboard">
                        <Pick
                          value={String(step.config.dashboardId ?? '')}
                          onChange={(value) => setConfig(index, 'dashboardId', value)}
                          options={[['', 'Choose a dashboard'], ...(dashboards.data ?? []).map((item): [string, string] => [item.id, item.name])]}
                          label="Dashboard"
                        />
                      </FormField>
                    )}
                    {step.kind === 'threshold' && (
                      <div className="grid grid-cols-3 gap-3">
                        <FormField label="Column">
                          <Input value={String(step.config.column ?? '')} onChange={(event) => setConfig(index, 'column', event.target.value)} />
                        </FormField>
                        <FormField label="Alert when">
                          <Pick
                            value={String(step.config.op ?? 'lt')}
                            onChange={(value) => setConfig(index, 'op', value)}
                            options={[['lt', 'below'], ['lte', 'at or below'], ['gt', 'above'], ['gte', 'at or above'], ['eq', 'equal']]}
                            label="Operator"
                          />
                        </FormField>
                        <FormField label="Value">
                          <Input value={String(step.config.value ?? '')} onChange={(event) => setConfig(index, 'value', Number(event.target.value))} />
                        </FormField>
                      </div>
                    )}
                    {step.kind === 'report' && (
                      <>
                        <FormField label="Output">
                          <Pick
                            value={String(step.config.artifact ?? 'pdf')}
                            onChange={(value) => setConfig(index, 'artifact', value)}
                            options={[['pdf', 'PDF report'], ['deck', 'Presentation']]}
                            label="Artifact"
                          />
                        </FormField>
                        <FormField label="What to generate">
                          <Textarea value={String(step.config.prompt ?? '')} onChange={(event) => setConfig(index, 'prompt', event.target.value)} rows={2} />
                        </FormField>
                      </>
                    )}
                    {step.kind === 'email' && (
                      <FormField label="Send to">
                        <Input value={String(step.config.to ?? '')} onChange={(event) => setConfig(index, 'to', event.target.value)} placeholder="you@example.com" />
                      </FormField>
                    )}
                    {step.kind === 'mcp_tool' && (
                      <>
                        <div className="grid grid-cols-2 gap-3">
                          <FormField label="Server">
                            <Pick
                              value={String(step.config.serverId ?? '')}
                              onChange={(value) => setConfig(index, 'serverId', value)}
                              options={[['', 'Choose a server'], ...(mcpServers.data ?? []).map((server): [string, string] => [server.id, server.name])]}
                              label="MCP server"
                            />
                          </FormField>
                          <FormField label="Tool">
                            <Input value={String(step.config.tool ?? '')} onChange={(event) => setConfig(index, 'tool', event.target.value)} />
                          </FormField>
                        </div>
                        <FormField label="Arguments (JSON)" hint='Write-like tools stay blocked unless allowed for the "automations" agent. Results are untrusted.'>
                          <Textarea
                            value={typeof step.config.args === 'string' ? step.config.args : step.config.args ? JSON.stringify(step.config.args) : ''}
                            onChange={(event) => setConfig(index, 'args', event.target.value)}
                            rows={2}
                            className="font-mono"
                            placeholder='{"limit": 10}'
                          />
                        </FormField>
                      </>
                    )}
                    {step.kind === 'webhook' && (
                      <>
                        <FormField label="URL" hint="Posts the previous result as JSON. Blocked offline unless the URL is on this device.">
                          <Input value={String(step.config.url ?? '')} onChange={(event) => setConfig(index, 'url', event.target.value)} placeholder="https://example.com/hook" />
                        </FormField>
                        <FormField label="Secret header (optional)">
                          <Input
                            type="password"
                            value={stepSecrets[step.key] ?? ''}
                            onChange={(event) => setStepSecrets((current) => ({ ...current, [step.key]: event.target.value }))}
                            placeholder={step.config.secret ? 'Saved in keychain — type to replace' : ''}
                          />
                        </FormField>
                      </>
                    )}
                    {step.kind === 'write_file' && (
                      <>
                        <FormField label="Folder">
                          <Input value={String(step.config.dir ?? '')} onChange={(event) => setConfig(index, 'dir', event.target.value)} placeholder="/Users/you/Documents/exports" />
                        </FormField>
                        <div className="grid grid-cols-[8rem_1fr] gap-3">
                          <FormField label="Format">
                            <Pick
                              value={String(step.config.format ?? 'csv')}
                              onChange={(value) => setConfig(index, 'format', value)}
                              options={[['csv', 'CSV'], ['json', 'JSON']]}
                              label="Format"
                            />
                          </FormField>
                          <FormField label="File name (optional)">
                            <Input value={String(step.config.filename ?? '')} onChange={(event) => setConfig(index, 'filename', event.target.value)} />
                          </FormField>
                        </div>
                        <div className="flex items-center gap-2">
                          <Checkbox
                            id={`overwrite-${index}`}
                            checked={Boolean(step.config.overwrite)}
                            onCheckedChange={(value) => setConfig(index, 'overwrite', value === true)}
                          />
                          <Label htmlFor={`overwrite-${index}`} className="text-app-caption font-normal">
                            Overwrite instead of adding a timestamp
                          </Label>
                        </div>
                      </>
                    )}
                    {step.kind === 'notify' && (
                      <div className="grid grid-cols-2 gap-3">
                        <FormField label="Title (optional)">
                          <Input value={String(step.config.title ?? '')} onChange={(event) => setConfig(index, 'title', event.target.value)} />
                        </FormField>
                        <FormField label="Message" hint="Defaults to the result summary.">
                          <Input value={String(step.config.message ?? '')} onChange={(event) => setConfig(index, 'message', event.target.value)} />
                        </FormField>
                        <div className="col-span-2 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                          <Button type="button" size="sm" variant="outline" onClick={() => api.notifyTest().then(() => toast.success('Test sent. Nothing showed up? Check notification settings.'), (failure) => toast.error(errorMessage(failure)))}>
                            Send test
                          </Button>
                          <Button type="button" size="sm" variant="ghost" onClick={() => void api.notifyOpenSettings()}>
                            Open notification settings
                          </Button>
                          <span>Alerts are hidden if notifications are off for LocalBoard or Focus is on.</span>
                        </div>
                      </div>
                    )}
                    {step.kind === 'branch' && (
                      <div className="grid grid-cols-[1fr_6rem_6rem] gap-3">
                        <FormField label="Continue only if">
                          <Pick
                            value={String(step.config.if ?? 'alert_fired')}
                            onChange={(value) => setConfig(index, 'if', value)}
                            options={[['alert_fired', 'The alert or threshold fired'], ['row_count', 'Row count is…']]}
                            label="Condition"
                          />
                        </FormField>
                        {step.config.if === 'row_count' && (
                          <>
                            <FormField label="Operator">
                              <Pick
                                value={String(step.config.op ?? '>')}
                                onChange={(value) => setConfig(index, 'op', value)}
                                options={['>', '>=', '<', '<=', '==', '!='].map((op): [string, string] => [op, op])}
                                label="Operator"
                              />
                            </FormField>
                            <FormField label="Value">
                              <Input type="number" value={String(step.config.value ?? 0)} onChange={(event) => setConfig(index, 'value', Number(event.target.value))} />
                            </FormField>
                          </>
                        )}
                      </div>
                    )}
                    {['compare', 'anomaly'].includes(step.kind) && <p className="text-app-caption text-muted-foreground">No settings. Uses the previous step’s result.</p>}
                  </div>,
                }))}
              />
            )}
          </section>
          <SaveBar
            dirty={dirty && Boolean(name.trim())}
            saving={save.isPending}
            label={creating ? 'New job, not saved yet' : 'Unsaved changes'}
            onSave={() => save.mutate()}
            onDiscard={() => (job ? load(job) : setCreating(false))}
          />
        </TabsContent>
        <TabsContent value="history" className="space-y-3">
          {runRows.length === 0 ? (
            <EmptyState icon={Repeat} title="No runs yet" description="Run it now or wait for the trigger." />
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>When</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Trigger</TableHead>
                  <TableHead>Summary</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {runPg.pageItems.map((entry) => (
                  <RunRow key={entry.id} entry={entry} steps={job?.steps ?? []} />
                ))}
              </TableBody>
            </Table>
          )}
          <PaginationBar page={runPg.page} pageSize={runPg.pageSize} total={runPg.total} onPageChange={runPg.setPage} onPageSizeChange={runPg.setPageSize} className="pt-1" />
        </TabsContent>
      </Tabs>
    </div>
  )

  return (
    <div>
      {selected ? (
        <>
          <DetailHeader title={<span className="text-app-label text-muted-foreground">Automations</span>} onBack={back} />
          {detail}
        </>
      ) : jobs.length === 0 ? (
        welcome
      ) : (
        <PageContainer width="full">
          <PageHeader
            title="Automations"
            description="Jobs run on a schedule, when a file changes, or on demand, and keep running after you close the window."
            actions={
              <>
                <Button variant="outline" onClick={() => setDraftOpen(true)}>
                  <Sparkles strokeWidth={iconStroke} /> Draft with AI
                </Button>
                <Button onClick={reset}>
                  <Plus strokeWidth={iconStroke} /> New job
                </Button>
              </>
            }
          />
          {jobList}
        </PageContainer>
      )}

      <Sheet open={creating} onOpenChange={(open) => !open && back()}>
        <SheetContent width="xl">
          <SheetHeader title="New job" description="Pick when it runs and what it does. Save, then turn it on." />
          <div className="min-h-0 flex-1 overflow-y-auto">{creating && detail}</div>
        </SheetContent>
      </Sheet>

      <Sheet open={pickerOpen} onOpenChange={setPickerOpen}>
        <SheetContent width="xl">
          <SheetHeader title="Add a step" description="Steps run in order. Each one receives the previous step’s result." />
          <SheetBody>
          <div className="space-y-5">
            {STEP_GROUPS.map((group) => (
              <div key={group.label} className="space-y-2">
                <p className="text-app-caption font-medium text-muted-foreground">{group.label}</p>
                <ChoiceGrid
                  columns={4}
                  onSelect={(kind) => {
                    setSteps((current) => [...current, emptyStep(kind)])
                    setPickerOpen(false)
                  }}
                  options={group.kinds.map((kind) => ({ value: kind, title: stepLabel(kind), description: STEP_HELP[kind], icon: STEP_ICON[kind] ?? Timer }))}
                />
              </div>
            ))}
          </div>
        
          </SheetBody>
        </SheetContent>
      </Sheet>

      <Sheet open={draftOpen} onOpenChange={setDraftOpen}>
        <SheetContent width="lg">
          <SheetHeader title="Draft with AI" description="Describe the job. The analyst drafts it and you review before it can run." />
          <SheetBody>
          <Textarea value={idea} onChange={(event) => setIdea(event.target.value)} rows={4} placeholder="Every Monday, email me revenue by region if it dropped more than 10%" aria-label="Describe the job" />
          <div className="flex flex-wrap gap-2">
            {IDEAS.map((text) => (
              <button key={text} type="button" onClick={() => setIdea(text)} className="rounded-full border border-border px-3 py-1 text-app-caption text-muted-foreground hover:text-foreground">
                {text}
              </button>
            ))}
          </div>
          <GenerationSteps running={propose.isPending} />
          <div className="flex justify-end gap-2">
            <Button variant="outline" onClick={() => setDraftOpen(false)}>
              Cancel
            </Button>
            <Button loading={propose.isPending} disabled={!idea.trim()} onClick={() => propose.mutate()}>
              {!propose.isPending && <Sparkles strokeWidth={iconStroke} />}
              Draft
            </Button>
          </div>
        
          </SheetBody>
        </SheetContent>
      </Sheet>

      <ConfirmDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={`Delete “${name || 'this job'}”?`}
        description="The job and its run history are removed. This cannot be undone."
        onConfirm={() => remove.mutate()}
      />
    </div>
  )
}

function RunRow({ entry, steps }: { entry: AutomationRun; steps: Automation['steps'] }) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <TableRow className="cursor-pointer hover:bg-accent/50" aria-expanded={open} onClick={() => setOpen((current) => !current)}>
        <TableCell className="whitespace-nowrap text-app-caption text-muted-foreground">{new Date(entry.startedAt).toLocaleString()}</TableCell>
        <TableCell>
          <Badge variant={statusVariant(entry.status)}>{entry.status}</Badge>
          {entry.notified && <Badge variant="brand" className="ml-1">notified</Badge>}
        </TableCell>
        <TableCell className="text-app-caption text-muted-foreground">{entry.triggerKind ?? '—'}</TableCell>
        <TableCell className="max-w-0 truncate text-app-caption text-muted-foreground">{entry.summary ?? '—'}</TableCell>
      </TableRow>
      {open && (
        <TableRow>
            <TableCell colSpan={4} className="space-y-2 bg-muted/30">
              {entry.steps.map((step) => {
                const config = steps.find((item) => item.id === step.stepId)?.config
                return (
                  <div key={step.id} className="space-y-1">
                    <p className="flex items-center gap-2 text-app-caption">
                      <Badge variant={statusVariant(step.status)}>{step.status}</Badge>
                      <span className="font-medium text-foreground">{step.kind}</span>
                      {step.summary && <span className="truncate text-muted-foreground">{step.summary}</span>}
                    </p>
                    <pre className="max-h-48 overflow-auto whitespace-pre-wrap rounded-lg bg-muted p-2 text-app-caption text-muted-foreground">
                      {`Input: ${JSON.stringify(config ?? {}, null, 2)}\nOutput: ${JSON.stringify(step.facts ?? step.summary, null, 2)}`}
                    </pre>
                  </div>
                )
              })}
              {entry.steps.length === 0 && <p className="text-app-caption text-muted-foreground">No step details.</p>}
            </TableCell>
        </TableRow>
      )}
    </>
  )
}
