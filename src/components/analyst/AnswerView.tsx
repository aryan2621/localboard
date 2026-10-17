import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { AlertTriangle, BookmarkPlus, Check, ChevronDown, Code2, GitBranch, Loader2, RotateCw, SquareTerminal, Square, ThumbsDown, ThumbsUp, X } from 'lucide-react'
import { toast } from 'sonner'
import {
  api,
  errorMessage,
  type Anomaly,
  type ChartSpec,
  type Driver,
  type Feedback,
  type MessageRecord,
  type QueryRecord,
  type Step,
} from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { cn } from '../../lib/utils'
import type { LiveAnswer } from '../../stores/analyst'
import { useNavigation } from '../../stores/navigation'
import { DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator } from '../ui/dropdown-menu'
import { Alert, AlertDescription } from '../ui/alert'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '../ui/collapsible'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../ui/table'
import { Hint } from '../ui/tooltip'
import { PaginationBar } from '../ui/pagination'
import { usePagination } from '../../lib/usePagination'
import { formatCell } from '../data/SampleTable'
import { MoreMenu, PageLoader, StatusBadge } from '../layout/ui'
import { Button } from '../ui/button'
import { formatNumber, ResultChart } from './ResultChart'
import { StepList, StepsPanel } from './StepsPanel'
import { Sheet, SheetBody, SheetContent, SheetHeader } from '../ui/sheet'

const RULE_LABEL: Record<Anomaly['rule'], string | null> = {
  zscore: 'z-score',
  iqr: null,
  both: 'answer.rule.both',
}

const PURPOSE_LABEL: Record<QueryRecord['purpose'], string> = {
  answer: 'answer.purpose.answer',
  comparison: 'answer.purpose.comparison',
  breakdown: 'answer.purpose.breakdown',
}

function signed(value: number) {
  return `${value > 0 ? '+' : ''}${formatNumber(value)}`
}

function Drivers({ drivers }: { drivers: Driver[] }) {
  const pg = usePagination(drivers, { initialPageSize: 10, resetKey: drivers })
  return (
    <div>
    <Table className="text-app-caption">
      <TableHeader>
        <TableRow>
          {(['Driver', 'Before', 'After', 'Change', 'Share of change'] as const).map((label) => (
            <TableHead key={label}>{label}</TableHead>
          ))}
        </TableRow>
      </TableHeader>
      <TableBody>
        {pg.pageItems.map((driver) => (
          <TableRow key={`${driver.dimension}-${formatCell(driver.segment)}`} className="tabular-nums">
            <TableCell className="py-2">
              <span className="text-muted-foreground">{driver.dimension} </span>
              <span className="font-mono text-foreground">{formatCell(driver.segment)}</span>
            </TableCell>
            <TableCell className="py-2">{formatNumber(driver.previous)}</TableCell>
            <TableCell className="py-2">{formatNumber(driver.current)}</TableCell>
            <TableCell className={cn('py-2', driver.change < 0 ? 'text-destructive' : 'text-success')}>{signed(driver.change)}</TableCell>
            <TableCell className="py-2">{driver.shareOfChange === null ? '—' : `${Math.round(driver.shareOfChange * 100)}%`}</TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
    <PaginationBar page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} className="pt-1" />
    </div>
  )
}

function Anomalies({ anomalies }: { anomalies: Anomaly[] }) {
  const pg = usePagination(anomalies, { initialPageSize: 10, resetKey: anomalies })
  return (
    <div>
    <Table className="text-app-caption">
      <caption className="px-3 py-2 text-left text-app-caption text-muted-foreground">{'Unusual periods, flagged in code'}</caption>
      <TableHeader>
        <TableRow>
          {(['Period', 'Measure', 'Value', 'z-score', 'Rule'] as const).map((label) => (
            <TableHead key={label}>{label}</TableHead>
          ))}
        </TableRow>
      </TableHeader>
      <TableBody>
        {pg.pageItems.map((anomaly) => (
          <TableRow key={`${anomaly.measure}-${formatCell(anomaly.period)}`} className="tabular-nums">
            <TableCell className="py-2 font-mono">{formatCell(anomaly.period)}</TableCell>
            <TableCell className="py-2">{anomaly.measure}</TableCell>
            <TableCell className="py-2">{formatNumber(anomaly.value)}</TableCell>
            <TableCell className="py-2">{formatNumber(anomaly.zScore)}</TableCell>
            <TableCell className="py-2">{RULE_LABEL[anomaly.rule] ? RULE_LABEL[anomaly.rule] : 'IQR'}</TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
    <PaginationBar page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} className="pt-1" />
    </div>
  )
}

function QueryStatusBadge({ query }: { query: QueryRecord }) {
  if (query.status === 'ok') return <StatusBadge variant="success">{`${(query.rowCount ?? 0).toLocaleString()} rows`}</StatusBadge>
  if (query.status === 'denied') return <StatusBadge variant="warning">{'Blocked'}</StatusBadge>
  return <StatusBadge variant="error">{'Failed'}</StatusBadge>
}

/** Every query behind the answer, in order, with the tables each one read. */
function SqlPanel({ queries, primaryId }: { queries: QueryRecord[]; primaryId: string | null }) {
  const openBench = useNavigation((state) => state.openBench)
  return (
    <div className="space-y-2">
      {queries.map((query) => (
        <div
          key={query.id}
          className={cn('rounded-xl border bg-surface', query.id === primaryId ? 'border-brand/40' : 'border-border')}
        >
          <div className="flex flex-wrap items-center gap-2 border-b border-border px-3 py-2 text-app-caption text-muted-foreground">
            <span className="text-foreground">
              {`${PURPOSE_LABEL[query.purpose]} · attempt ${query.attempt}`}
            </span>
            <QueryStatusBadge query={query} />
            {query.durationMs !== null && <span>{query.durationMs.toLocaleString()} ms</span>}
            {query.relations.length > 0 && (
              <span className="flex flex-wrap items-center gap-1">
                {'Reads'}
                {query.relations.map((relation) => (
                  <span key={relation} className="rounded-md bg-card px-1.5 py-0.5 font-mono text-app-caption text-foreground">
                    {relation}
                  </span>
                ))}
              </span>
            )}
            <Button variant="ghost" size="sm" className="ml-auto h-7" onClick={() => openBench(query.sql)}>
              <SquareTerminal strokeWidth={iconStroke} /> {'Edit in Query Bench'}
            </Button>
          </div>
          <pre className="overflow-x-auto whitespace-pre-wrap px-3 py-2 font-mono text-app-caption text-foreground">{query.sql}</pre>
          {query.error && <p className="border-t border-border px-3 py-2 text-app-caption text-destructive">{query.error}</p>}
        </div>
      ))}
    </div>
  )
}

function MemoryBanner({ proposal, agentId }: { proposal: { id: string; body: string }; agentId: string }) {
  const { data: memories } = useQuery({ queryKey: queryKeys.memories(agentId), queryFn: () => api.memories(agentId) })
  const status = useMutation({
    mutationFn: (next: 'active' | 'archived') => api.setMemoryStatus(proposal.id, next),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.memories(agentId) }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  if (memories?.find((memory) => memory.id === proposal.id)?.status !== 'proposed') return null
  return (
    <div className="rounded-xl border border-border bg-card px-3 py-2.5">
      <p className="text-app-caption text-muted-foreground">{'Remember this for future questions?'}</p>
      <p className="mt-1 text-app-label text-foreground">{proposal.body}</p>
      <div className="mt-2 flex gap-2">
        <Button size="sm" onClick={() => status.mutate('active')} disabled={status.isPending}>
          {'Save'}
        </Button>
        <Button variant="ghost" size="sm" onClick={() => status.mutate('archived')} disabled={status.isPending}>
          {'Dismiss'}
        </Button>
      </div>
    </div>
  )
}

/** What the backend recorded about an answer: tables read, their sources, the model and the analyst. */
function LineagePanel({ message, queries }: { message: MessageRecord; queries: QueryRecord[] }) {
  const answer = message.answer
  const lineage = answer?.lineage
  const { data: tables } = useQuery({ queryKey: queryKeys.tables, queryFn: api.tables })
  const { data: connections } = useQuery({ queryKey: queryKeys.connections, queryFn: api.connections })
  const relations = [...new Set(queries.flatMap((query) => query.relations))]
  const primary = queries.find((query) => query.id === answer?.primaryQueryId) ?? queries.find((query) => query.status === 'ok')
  const columns = primary?.result?.columns ?? []
  return (
    <div className="space-y-3 rounded-xl border border-border bg-surface px-3 py-2.5 text-app-caption">
      <dl className="flex flex-wrap gap-x-6 gap-y-1">
        <div className="flex gap-1.5">
          <dt className="text-muted-foreground">{'Model'}</dt>
          <dd className="text-foreground">{answer?.model ? `${answer.model}${lineage?.modelId ? ` (${lineage.modelId})` : ''}` : 'None (SQL only)'}</dd>
        </div>
        {answer?.agentName && (
          <div className="flex gap-1.5">
            <dt className="text-muted-foreground">{'Analyst'}</dt>
            <dd className="text-foreground">{answer.agentName}</dd>
          </div>
        )}
      </dl>
      <div className="space-y-1">
        <p className="font-medium text-foreground">{'Tables read'}</p>
        {relations.length === 0 && <p className="text-muted-foreground">{'No tables were recorded for this answer.'}</p>}
        <ul className="space-y-1">
          {relations.map((relation) => {
            const table = tables?.find((item) => item.relation === relation)
            const connection = table ? connections?.find((item) => item.id === table.connection_id) : undefined
            return (
              <li key={relation} className="flex flex-wrap items-center gap-1.5">
                <span className="rounded-md bg-card px-1.5 py-0.5 font-mono text-app-caption text-foreground">{relation}</span>
                <span className="text-muted-foreground">
                  {connection ? `${'Source'}: ${connection.name} · ${table?.source_ref}` : 'Source not found'}
                </span>
              </li>
            )
          })}
        </ul>
      </div>
      {columns.length > 0 && (
        <div className="space-y-1">
          <p className="font-medium text-foreground">{'Columns in the result'}</p>
          <p className="flex flex-wrap gap-1">
            {columns.map((column) => (
              <span key={column.name} className="rounded-md bg-card px-1.5 py-0.5 font-mono text-app-caption text-foreground">{column.name}</span>
            ))}
          </p>
        </div>
      )}
      {lineage ? (
        <div className="space-y-1">
          <p className="font-medium text-foreground">{'Context given to the model'}</p>
          <dl className="space-y-1">
            {[
              ['Rules', lineage.rules.map((rule) => `${rule.title || rule.id.slice(0, 8)} · v ${rule.version}`)],
              ['Examples', lineage.examples.map((id) => id.slice(0, 8))],
              ['Memories', lineage.memories.map((id) => id.slice(0, 8))],
              ['Knowledge excerpts', lineage.knowledge.map((chunk) => `${chunk.doc} #${chunk.chunk}`)],
              ['Columns used in the SQL', lineage.columns],
            ].map(([label, items]) => (
              <div key={label as string} className="flex flex-wrap items-baseline gap-1.5">
                <dt className="text-muted-foreground">{label as string}</dt>
                {(items as string[]).length === 0 ? (
                  <dd className="text-muted-foreground">{'None'}</dd>
                ) : (
                  (items as string[]).map((item) => (
                    <dd key={item} className="rounded-md bg-card px-1.5 py-0.5 font-mono text-app-caption text-foreground">{item}</dd>
                  ))
                )}
              </div>
            ))}
          </dl>
        </div>
      ) : (
        <p className="text-muted-foreground">{'Rules, examples, memories and knowledge used for this answer were not recorded because it was saved before lineage tracking.'}</p>
      )}
    </div>
  )
}

function refresh(conversationId: string) {
  return queryClient.invalidateQueries({ queryKey: queryKeys.conversation(conversationId) })
}

export function AnswerView({
  message,
  queries,
  live,
  onAsk,
  canAsk,
}: {
  message: MessageRecord
  queries: QueryRecord[]
  live: LiveAnswer | undefined
  onAsk: (question: string) => void
  canAsk: boolean
}) {
  const [showSql, setShowSql] = useState(false)
  const [showLineage, setShowLineage] = useState(false)
  const answer = message.answer
  const pending = message.status === 'pending'
  const steps = pending ? (live?.steps ?? []) : (answer?.steps ?? [])
  const text = pending ? (live?.text ?? '') : message.content
  const charts: { title: string | null; spec: ChartSpec }[] =
    answer?.parts && answer.parts.length > 0
      ? answer.parts.map((part) => ({ title: part.question, spec: part.chart }))
      : answer?.chart
        ? [{ title: null, spec: answer.chart }]
        : []
  const rerunIds = [...new Set([answer?.primaryQueryId, ...charts.map((chart) => chart.spec.queryId)].filter((id): id is string => Boolean(id)))]

  const stop = useMutation({
    mutationFn: () => api.cancelAnswer(message.id),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const rerun = useMutation({
    mutationFn: () => Promise.all(rerunIds.map((id) => api.rerunQuery(id))),
    onSuccess: async (records) => {
      await refresh(message.conversationId)
      const failed = records.find((record) => record.status !== 'ok')
      if (failed) toast.error(failed.error ?? 'The query failed')
      else toast.success('Updated with the latest data')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const feedback = useMutation({
    mutationFn: (value: Feedback | null) => api.setFeedback(message.id, value),
    onSuccess: () => refresh(message.conversationId),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const vote = (value: Feedback) => feedback.mutate(message.feedback === value ? null : value)
  const saveExample = useMutation({
    mutationFn: () => api.saveAnswerExample(message.id),
    onSuccess: async () => {
      await refresh(message.conversationId)
      toast.success('Saved as an example for this analyst')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const primaryOk = queries.some((query) => query.id === answer?.primaryQueryId && query.status === 'ok')
  const kept = queries.some((query) => query.status === 'ok')
  const openDatasite = useNavigation((state) => state.openDatasite)
  const openReport = useNavigation((state) => state.openReport)
  const openDeck = useNavigation((state) => state.openDeck)
  const convert = useMutation({
    mutationFn: async (kind: 'datasite' | 'report' | 'deck') => {
      if (kind === 'datasite') {
        const live = await api.datasiteFromAnswer(message.id)
        return { kind, id: live.datasite.id, warnings: live.warnings }
      }
      if (kind === 'report') {
        const live = await api.reportFromAnswer(message.id)
        return { kind, id: live.report.id, warnings: live.warnings }
      }
      const live = await api.deckFromAnswer(message.id)
      return { kind, id: live.deck.id, warnings: live.warnings }
    },
    onSuccess: ({ kind, id, warnings }) => {
      if (warnings.length) toast.message(warnings.join(' '))
      if (kind === 'datasite') {
        void queryClient.invalidateQueries({ queryKey: queryKeys.datasites })
        openDatasite(id)
        return
      }
      if (kind === 'report') {
        void queryClient.invalidateQueries({ queryKey: queryKeys.reports })
        openReport(id)
        return
      }
      void queryClient.invalidateQueries({ queryKey: queryKeys.decks })
      openDeck(id)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  return (
    <div className="space-y-4">
      {pending && (
        <div className="space-y-3">
          {steps.length > 0 ? (
            <StepsPanel steps={steps} running />
          ) : (
            <PageLoader label={'Working…'} />
          )}
        </div>
      )}
      {!pending && steps.length > 0 && (
        <Collapsible className="group text-app-label">
          <CollapsibleTrigger className="flex items-center gap-1 text-app-caption text-muted-foreground hover:text-foreground">
            <ChevronDown size={14} strokeWidth={iconStroke} className="-rotate-90 transition-transform group-data-[state=open]:rotate-0" />
            {steps.length} {steps.length === 1 ? 'step' : 'steps'}
            {answer?.model ? ` · ${answer.model}` : answer?.mode === 'sql' ? ` · ${'SQL only'}` : ''}
          </CollapsibleTrigger>
          <CollapsibleContent className="mt-2 pl-5">
            <StepList steps={steps} />
          </CollapsibleContent>
        </Collapsible>
      )}

      {message.status === 'error' ? (
        <Alert variant="destructive">
          <AlertDescription className="whitespace-pre-wrap">
            {steps.find((step) => step.status === 'error') && (
              <span className="mb-1 block font-medium">Failed at: {steps.find((step) => step.status === 'error')?.label}</span>
            )}
            {text}
          </AlertDescription>
        </Alert>
      ) : (
        text && (
          <p className={cn('whitespace-pre-wrap text-app-body leading-relaxed', message.status === 'cancelled' ? 'text-muted-foreground' : 'text-foreground')}>
            {text}
          </p>
        )
      )}

      {answer?.clarification && answer.clarification.options.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {answer.clarification.options.map((option) => (
            <Button key={option} variant="outline" size="sm" disabled={!canAsk} onClick={() => onAsk(option)}>
              {option}
            </Button>
          ))}
        </div>
      )}

      {charts.map(({ title, spec }) => {
        const result = queries.find((query) => query.id === spec.queryId)?.result
        if (!result) return null
        return (
          <div key={spec.queryId} className="space-y-2">
            {title && <p className="text-app-label text-foreground">{title}</p>}
            <ResultChart spec={spec} result={result} />
          </div>
        )
      })}
      {answer && answer.drivers.length > 0 && <Drivers drivers={answer.drivers} />}
      {answer?.anomalies && answer.anomalies.length > 0 && <Anomalies anomalies={answer.anomalies} />}
      {answer?.memoryProposal && answer.agentId && (
        <MemoryBanner proposal={answer.memoryProposal} agentId={answer.agentId} />
      )}

      {answer && (answer.assumptions.length > 0 || answer.warnings.length > 0) && (
        <ul className="space-y-1 text-app-caption text-muted-foreground">
          {answer.assumptions.map((assumption) => (
            <li key={assumption}>{`Assumed: ${assumption}`}</li>
          ))}
          {answer.warnings.map((warning) => (
            <li key={warning} className="flex gap-1.5 text-warning">
              <AlertTriangle size={13} strokeWidth={iconStroke} className="mt-px shrink-0" /> {warning}
            </li>
          ))}
        </ul>
      )}

      <>
        <div className="flex w-fit flex-wrap items-center gap-0.5 rounded-lg border border-border bg-surface p-0.5">
          {pending ? (
            <Button variant="outline" size="sm" onClick={() => stop.mutate()} disabled={stop.isPending}>
              <Square strokeWidth={iconStroke} /> {'Stop'}
            </Button>
          ) : (
            <>
              {queries.length > 0 && (
                <Button variant="ghost" size="sm" onClick={() => setShowSql(true)}>
                  <Code2 strokeWidth={iconStroke} /> SQL
                </Button>
              )}
              {rerunIds.length > 0 && (
                <Button variant="ghost" size="sm" onClick={() => rerun.mutate()} disabled={rerun.isPending}>
                  <RotateCw strokeWidth={iconStroke} className={rerun.isPending ? 'animate-spin' : undefined} /> {'Re-run'}
                </Button>
              )}
              {message.status === 'done' && (
                <>
                  <Hint text="Good answer">
                    <Button
                      variant="ghost"
                      size="icon"
                      className={cn(message.feedback === 1 && 'text-success')}
                      aria-label={'Good answer'}
                      aria-pressed={message.feedback === 1}
                      onClick={() => vote(1)}
                    >
                      <ThumbsUp strokeWidth={iconStroke} />
                    </Button>
                  </Hint>
                  <Hint text="Bad answer">
                    <Button
                      variant="ghost"
                      size="icon"
                      className={cn(message.feedback === -1 && 'text-destructive')}
                      aria-label={'Bad answer'}
                      aria-pressed={message.feedback === -1}
                      onClick={() => vote(-1)}
                    >
                      <ThumbsDown strokeWidth={iconStroke} />
                    </Button>
                  </Hint>
                </>
              )}
              {(queries.length > 0 || (message.status === 'done' && (primaryOk || kept))) && (
                <MoreMenu>
                  {queries.length > 0 && (
                    <DropdownMenuItem onSelect={() => setShowLineage((open) => !open)}>
                      <GitBranch size={14} strokeWidth={iconStroke} /> {showLineage ? 'Hide lineage' : 'Show lineage'}
                    </DropdownMenuItem>
                  )}
                  {message.status === 'done' && primaryOk && (
                    <DropdownMenuItem disabled={saveExample.isPending} onSelect={() => saveExample.mutate()}>
                      <BookmarkPlus size={14} strokeWidth={iconStroke} /> {'Save as example'}
                    </DropdownMenuItem>
                  )}
                  {message.status === 'done' && kept && (
                    <>
                      <DropdownMenuSeparator />
                      <DropdownMenuLabel>{'Save as'}</DropdownMenuLabel>
                      <DropdownMenuItem disabled={convert.isPending} onSelect={() => convert.mutate('datasite')}>{'Datasite'}</DropdownMenuItem>
                      <DropdownMenuItem disabled={convert.isPending} onSelect={() => convert.mutate('report')}>{'PDF report'}</DropdownMenuItem>
                      <DropdownMenuItem disabled={convert.isPending} onSelect={() => convert.mutate('deck')}>{'Presentation'}</DropdownMenuItem>
                    </>
                  )}
                </MoreMenu>
              )}
            </>
          )}
        </div>
        <Sheet open={showSql && !pending} onOpenChange={setShowSql}>
          <SheetContent width="lg">
            <SheetHeader title="How this was answered" description={`${queries.length} ${queries.length === 1 ? 'query' : 'queries'} · every attempt, its timing and the tables it read`} />
            <SheetBody>
              {answer?.explanation && <p className="rounded-lg bg-muted/40 px-3 py-2 text-app-label text-foreground">{answer.explanation}</p>}
              <SqlPanel queries={queries} primaryId={answer?.primaryQueryId ?? null} />
            </SheetBody>
          </SheetContent>
        </Sheet>
      </>

      <Collapsible open={showLineage && !pending} onOpenChange={setShowLineage}>
        <CollapsibleContent>
          <LineagePanel message={message} queries={queries} />
        </CollapsibleContent>
      </Collapsible>
    </div>
  )
}
