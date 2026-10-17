import { useEffect, useState } from 'react'
import { useInfiniteQuery, useMutation } from '@tanstack/react-query'
import { save } from '@tauri-apps/plugin-dialog'
import { AlertTriangle, Ban, CheckCircle2, Download, Gauge, ScrollText } from 'lucide-react'
import { toast } from 'sonner'
import { Badge } from '../../components/ui/badge'
import { Button } from '../../components/ui/button'
import { Sheet, SheetBody, SheetContent, SheetHeader } from '../../components/ui/sheet'
import { DropdownMenuItem } from '../../components/ui/dropdown-menu'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../../components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../../components/ui/table'
import { PaginationBar } from '../../components/ui/pagination'
import { Hint } from '../../components/ui/tooltip'
import { CopyField, EmptyState, MoreMenu, PageContainer, PageHeader, PageLoader, SearchInput } from '../../components/layout/ui'
import { FilterChips, StatRow } from '../../components/layout/patterns'
import { relativeTime } from '../../lib/time'
import { api, errorMessage, type AuditEntry, type AuditFilter } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { queryKeys } from '../../lib/queryClient'

/** `value` is sent to the API ('' = no filter); `all` is the Select sentinel because Radix items cannot be empty. */
const ALL = 'all'

const KINDS: readonly (readonly [string, string])[] = [
  ['', 'All activity'],
  ['analyst.query', 'Question asked'],
  ['analyst.rerun', 'Query re-run'],
  ['source.add', 'Source added'],
  ['source.refresh', 'Source refreshed'],
  ['connection.remove', 'Connection removed'],
  ['connection.test', 'Connection tested'],
  ['connection.save', 'Connection saved'],
  ['Run', 'Query Bench run'],
  ['dashboard.tile', 'Dashboard tile'],
  ['datasite.chart', 'Datasite chart'],
  ['report.block', 'Report block'],
  ['deck.slide', 'Deck slide'],
  ['automation.run', 'Automation run'],
]

const STATUSES: readonly (readonly [string, string])[] = [
  ['', 'Any status'],
  ['ok', 'OK'],
  ['error', 'Error'],
  ['denied', 'Denied'],
]

const SQL_CLIP = 96
const SEARCH_DEBOUNCE_MS = 200

function clip(sql: string | null) {
  if (!sql) return '—'
  const compact = sql.replace(/\s+/g, ' ').trim()
  return compact.length > SQL_CLIP ? `${compact.slice(0, SQL_CLIP)}…` : compact
}

function kindLabel(kind: string) {
  return KINDS.find(([value]) => value === kind)?.[1] ?? kind
}

function StatusPill({ status }: { status: string }) {
  const variant = status === 'ok' ? 'success' : status === 'error' ? 'destructive' : status === 'denied' ? 'warning' : 'secondary'
  return <Badge variant={variant}>{status === 'ok' ? 'OK' : status.charAt(0).toUpperCase() + status.slice(1)}</Badge>
}

function StatusDot({ status }: { status: string }) {
  const tone = status === 'ok' ? 'bg-success' : status === 'error' ? 'bg-destructive' : status === 'denied' ? 'bg-warning' : 'bg-muted-foreground'
  return <span className={`inline-block size-2 shrink-0 rounded-full ${tone}`} aria-hidden />
}

function formatDetail(detail: unknown) {
  if (detail == null || detail === '') return '—'
  try {
    return JSON.stringify(detail, null, 2)
  } catch {
    return String(detail)
  }
}

export function AuditPage() {
  const [kind, setKind] = useState('')
  const [status, setStatus] = useState('')
  const [draft, setDraft] = useState('')
  const [search, setSearch] = useState('')
  const [selected, setSelected] = useState<AuditEntry | null>(null)

  useEffect(() => {
    const timer = window.setTimeout(() => setSearch(draft.trim()), SEARCH_DEBOUNCE_MS)
    return () => window.clearTimeout(timer)
  }, [draft])

  const filter: AuditFilter = { kind, status, search }
  const list = useInfiniteQuery({
    queryKey: queryKeys.audit(kind, status, search),
    queryFn: ({ pageParam }) =>
      api.auditList({
        ...filter,
        beforeTs: pageParam?.ts,
        beforeId: pageParam?.id,
      }),
    initialPageParam: undefined as { ts: string; id: number } | undefined,
    getNextPageParam: (last) => {
      const tail = last.entries[last.entries.length - 1]
      return last.hasMore && tail ? { ts: tail.ts, id: tail.id } : undefined
    },
  })

  const entries = list.data?.pages.flatMap((page) => page.entries) ?? []

  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(10)
  useEffect(() => setPage(1), [kind, status, search, pageSize])
  const visible = entries.slice((page - 1) * pageSize, page * pageSize)

  // The log loads 100 rows at a time from the server; fetch ahead only when the requested page needs more.
  const goToPage = async (next: number) => {
    let loaded = entries.length
    let more = Boolean(list.hasNextPage)
    while (next * pageSize > loaded && more) {
      const result = await list.fetchNextPage()
      loaded = result.data?.pages.flatMap((entry) => entry.entries).length ?? loaded
      more = Boolean(result.hasNextPage)
    }
    setPage(Math.max(1, Math.min(next, Math.ceil(loaded / pageSize))))
  }

  useEffect(() => {
    if (selected && !entries.some((entry) => entry.id === selected.id)) {
      setSelected(null)
    }
  }, [entries, selected])

  const exportAs = useMutation({
    mutationFn: async (format: 'json' | 'csv') => {
      const path = await save({ defaultPath: `audit.${format}`, filters: [{ name: format, extensions: [format] }] })
      if (!path) return null
      return api.auditExport(filter, format, path)
    },
    onSuccess: (result) => {
      if (result) toast.success(`Exported ${result.count} ${result.count === 1 ? 'row' : 'rows'}`)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  return (
    <PageContainer width="full">
      <PageHeader
        title="Audit"
        description="Every query and change, newest first."
        actions={
          <MoreMenu label="Export">
            <DropdownMenuItem disabled={exportAs.isPending} onSelect={() => exportAs.mutate('json')}>
              <Download size={14} strokeWidth={iconStroke} /> Export JSON
            </DropdownMenuItem>
            <DropdownMenuItem disabled={exportAs.isPending} onSelect={() => exportAs.mutate('csv')}>
              <Download size={14} strokeWidth={iconStroke} /> Export CSV
            </DropdownMenuItem>
          </MoreMenu>
        }
      />
      <StatRow
        stats={[
          { label: 'Entries loaded', value: entries.length.toLocaleString(), hint: list.hasNextPage ? 'More load as you page' : 'All matching entries', icon: ScrollText },
          { label: 'OK', value: entries.filter((entry) => entry.status === 'ok').length, icon: CheckCircle2, tone: 'success', onClick: () => setStatus('ok') },
          { label: 'Errors', value: entries.filter((entry) => entry.status === 'error').length, icon: AlertTriangle, tone: 'destructive', onClick: () => setStatus('error') },
          { label: 'Denied', value: entries.filter((entry) => entry.status === 'denied').length, icon: Ban, tone: 'warning', onClick: () => setStatus('denied') },
          {
            label: 'Avg duration',
            value: (() => {
              const timed = entries.filter((entry) => entry.durationMs != null)
              return timed.length ? `${Math.round(timed.reduce((sum, entry) => sum + (entry.durationMs ?? 0), 0) / timed.length)} ms` : '—'
            })(),
            icon: Gauge,
          },
        ]}
      />
      <div className="flex flex-wrap items-center gap-3">
        <SearchInput value={draft} onChange={setDraft} placeholder="Search question or SQL" className="min-w-[16rem] flex-1" />
        <Select value={kind || ALL} onValueChange={(value) => setKind(value === ALL ? '' : value)}>
          <SelectTrigger className="w-52" aria-label="Kind">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {KINDS.map(([value, label]) => (
              <SelectItem key={value || ALL} value={value || ALL}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <FilterChips
          allLabel="Any status"
          value={status || null}
          onChange={(next) => setStatus(next ?? '')}
          options={STATUSES.filter(([value]) => value).map(([value, label]) => ({ value, label }))}
        />
      </div>
      {list.isLoading ? (
        <PageLoader />
      ) : entries.length === 0 ? (
        <EmptyState icon={ScrollText} title="No matching entries" description="Activity appears here as you ask questions and change connections." />
      ) : (
        <div className="space-y-3">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-44">Time</TableHead>
                <TableHead className="w-44">Activity</TableHead>
                <TableHead className="w-24">Status</TableHead>
                <TableHead>Query</TableHead>
                <TableHead className="w-20 text-right">Duration</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {visible.map((entry) => (
                <TableRow key={entry.id} className="cursor-pointer hover:bg-accent/50" onClick={() => setSelected(entry)}>
                  <TableCell className="whitespace-nowrap text-app-caption text-muted-foreground">
                    <Hint text={new Date(entry.ts).toLocaleString()}>
                      <span>{relativeTime(entry.ts)}</span>
                    </Hint>
                  </TableCell>
                  <TableCell className="whitespace-nowrap">
                    <span className="flex items-center gap-2">
                      <StatusDot status={entry.status} />
                      {kindLabel(entry.kind)}
                    </span>
                  </TableCell>
                  <TableCell>
                    <StatusPill status={entry.status} />
                  </TableCell>
                  <TableCell className="max-w-0">
                    <Hint text={entry.sql ? clip(entry.sql) : undefined}>
                      <span className="block truncate font-mono text-app-caption text-muted-foreground">{clip(entry.sql)}</span>
                    </Hint>
                  </TableCell>
                  <TableCell className="whitespace-nowrap text-right text-app-caption text-muted-foreground">
                    {entry.durationMs == null ? '—' : `${entry.durationMs} ms`}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
          <PaginationBar
            page={page}
            pageSize={pageSize}
            total={entries.length}
            hasMore={Boolean(list.hasNextPage)}
            onPageChange={(next) => void goToPage(next)}
            onPageSizeChange={setPageSize}
          />
        </div>
      )}
      <Sheet open={selected !== null} onOpenChange={(open) => !open && setSelected(null)}>
        <SheetContent width="lg">{selected && <AuditDetail entry={selected} />}</SheetContent>
      </Sheet>
    </PageContainer>
  )
}

function AuditDetail({ entry }: { entry: AuditEntry }) {
  return (
    <>
      <SheetHeader title={kindLabel(entry.kind)} description={`${new Date(entry.ts).toLocaleString()} · ${relativeTime(entry.ts)}`} />
      <SheetBody>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-3 rounded-xl border border-border bg-muted/30 p-4 text-app-label sm:grid-cols-3">
        <Field label="Status">
          <StatusPill status={entry.status} />
        </Field>
        <Field label="Actor">{entry.actor}</Field>
        <Field label="Duration">{entry.durationMs == null ? '—' : `${entry.durationMs} ms`}</Field>
        <Field label="Rows">{entry.rows == null ? '—' : String(entry.rows)}</Field>
        <Field label="Tables" className="col-span-2">
          {entry.relations.length === 0 ? (
            '—'
          ) : (
            <span className="flex flex-wrap gap-1">
              {entry.relations.map((relation) => (
                <Badge key={relation} variant="secondary" className="font-mono">
                  {relation}
                </Badge>
              ))}
            </span>
          )}
        </Field>
      </dl>
      {entry.question?.trim() && (
        <div className="space-y-2">
          <p className="text-app-caption font-medium text-muted-foreground">Question</p>
          <p className="rounded-lg border border-border bg-surface px-3 py-2 text-app-label text-foreground">{entry.question.trim()}</p>
        </div>
      )}
      {entry.sql?.trim() && (
        <div className="space-y-2">
          <p className="text-app-caption font-medium text-muted-foreground">SQL</p>
          <pre className="max-h-60 overflow-auto whitespace-pre-wrap rounded-lg border border-border bg-muted/40 p-3 font-mono text-app-caption">{entry.sql.trim()}</pre>
          <CopyField value={entry.sql.trim()} label="Copy SQL" />
        </div>
      )}
      {formatDetail(entry.detail) !== '—' && (
        <div className="space-y-2">
          <p className="text-app-caption font-medium text-muted-foreground">Details</p>
          <pre className="max-h-48 overflow-auto whitespace-pre-wrap rounded-lg border border-border bg-muted/40 p-3 font-mono text-app-caption">
            {formatDetail(entry.detail)}
          </pre>
        </div>
      )}
      </SheetBody>
    </>
  )
}

function Field({ label, children, className }: { label: string; children: React.ReactNode; className?: string }) {
  return (
    <div className={className}>
      <dt className="text-app-caption text-muted-foreground">{label}</dt>
      <dd className="mt-0.5 text-foreground">{children}</dd>
    </div>
  )
}
