import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { ArrowDownToLine, Check, Cpu, Gauge, Loader2, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { api, errorMessage, type Benchmark, type LocalModel, type ModelCatalog } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { Button } from '../ui/button'
import { StatusBadge } from '../layout/ui'

const gb = (mb: number) => `${(mb / 1000).toFixed(1)} GB`

function EmbeddingModel({
  catalog,
  downloading,
  busy,
  onDownload,
  onDelete,
}: {
  catalog: ModelCatalog
  downloading: boolean
  busy: boolean
  onDownload: () => void
  onDelete: () => void
}) {
  const model = catalog.embedding
  const active = downloading && catalog.download?.id === model.id
  return (
    <div className="rounded-xl border border-border bg-surface px-4 py-3.5">
      <div className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <p className="text-app-label font-medium text-foreground">{model.name}</p>
          <p className="mt-0.5 text-app-caption leading-relaxed text-muted-foreground">{model.note}</p>
          <p className="mt-0.5 text-app-caption text-muted-foreground">{model.sizeMb} MB</p>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {active ? (
            <DownloadBar catalog={catalog} />
          ) : model.installed ? (
            <Button type="button" variant="ghost" size="icon" onClick={onDelete} disabled={busy} aria-label={`Delete ${model.name}`}>
              <Trash2 strokeWidth={iconStroke} />
            </Button>
          ) : (
            <Button variant="outline" size="sm" disabled={downloading || busy} onClick={onDownload}>
              <ArrowDownToLine strokeWidth={iconStroke} /> Download
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}

function refresh() {
  return queryClient.invalidateQueries({ queryKey: queryKeys.models })
}

function DownloadBar({ catalog }: { catalog: ModelCatalog }) {
  const download = catalog.download
  const percent = download?.total ? Math.round((download.done / download.total) * 100) : 0
  return (
    <>
      <div
        className="h-1.5 w-28 overflow-hidden rounded-full bg-muted"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
      >
        <div className="h-full bg-brand transition-[width]" style={{ width: `${percent}%` }} />
      </div>
      <span className="w-9 text-right text-app-caption tabular-nums text-muted-foreground">{download?.total ? `${percent}%` : '…'}</span>
      <Button variant="ghost" size="sm" onClick={() => void api.cancelDownload()}>
        Cancel
      </Button>
    </>
  )
}

/**
 * The built-in models with download, use, speed test and delete. `only="recommended"` limits
 * the list for onboarding.
 */
export function ModelManager({ only }: { only?: 'recommended' }) {
  const { data: catalog, error } = useQuery({ queryKey: queryKeys.models, queryFn: api.modelCatalog })
  const [benchmarks, setBenchmarks] = useState<Record<string, Benchmark>>({})

  const action = useMutation({
    mutationFn: (run: () => Promise<unknown>) => run(),
    onSettled: () => void refresh(),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const benchmark = useMutation({
    mutationFn: api.benchmarkModel,
    onSuccess: (result) => {
      setBenchmarks((current) => ({ ...current, [result.modelId]: result }))
      void refresh()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  if (error) return <p className="text-app-label text-destructive">{errorMessage(error)}</p>
  if (!catalog) {
    return (
      <div className="flex justify-center py-10" role="status" aria-label="Loading models">
        <Loader2 size={20} strokeWidth={iconStroke} className="animate-spin text-muted-foreground" />
      </div>
    )
  }

  const downloading = Boolean(catalog.download && !catalog.download.finished && !catalog.download.error)
  const models = only === 'recommended' ? catalog.models.filter((m) => m.id === catalog.recommended) : catalog.models
  const recommended = catalog.models.find((m) => m.id === catalog.recommended)

  const use = (model: LocalModel) =>
    action.mutate(async () => {
      await api.setTaskRouting({ sql: model.id, reasoning: model.id, narrative: model.id })
      toast.success(`${model.name} is now used for analysis`)
    })

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2.5 rounded-xl bg-card px-4 py-3 text-app-label text-foreground">
        <Cpu size={16} strokeWidth={iconStroke} className="shrink-0 text-brand" />
        <span>
          This computer has <strong className="font-medium">{catalog.ramGb} GB</strong> of memory.
          {recommended ? (
            <>
              {' '}
              <strong className="font-medium">{recommended.name}</strong> is the best fit for it.
            </>
          ) : null}
        </span>
      </div>

      <div className="divide-y divide-border rounded-xl border border-border bg-surface">
        {models.map((m) => {
          const isDownloading = downloading && catalog.download?.id === m.id
          const tooBig = m.minRamGb > catalog.ramGb
          const result = benchmarks[m.id]
          const testing = benchmark.isPending && benchmark.variables === m.id
          return (
            <div key={m.id} className="flex items-center gap-4 px-4 py-3.5">
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-app-label font-medium text-foreground">{m.name}</span>
                  {m.id === catalog.recommended && <StatusBadge variant="brand">Recommended</StatusBadge>}
                  {m.installed && m.id === catalog.active && <StatusBadge variant="success">In use</StatusBadge>}
                </div>
                <p className="mt-0.5 text-app-caption leading-relaxed text-muted-foreground">{m.note}</p>
                <p className="mt-0.5 text-app-caption text-muted-foreground">
                  {gb(m.sizeMb)} · needs {m.minRamGb} GB of memory
                  {tooBig ? ' · more than this computer has' : ''}
                  {result ? ` · ${result.tokensPerSec.toFixed(1)} tokens/s, loaded in ${(result.loadMs / 1000).toFixed(1)}s` : ''}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-1.5">
                {isDownloading ? (
                  <DownloadBar catalog={catalog} />
                ) : m.installed ? (
                  <>
                    <Button
                      variant="ghost"
                      size="sm"
                      disabled={benchmark.isPending}
                      onClick={() => benchmark.mutate(m.id)}
                    >
                      {testing ? <Loader2 className="animate-spin" /> : <Gauge strokeWidth={iconStroke} />}
                      {testing ? 'Testing…' : 'Test speed'}
                    </Button>
                    {m.id !== catalog.active && (
                      <Button variant="outline" size="sm" onClick={() => use(m)}>
                        <Check strokeWidth={iconStroke} /> Use
                      </Button>
                    )}
                    <Button type="button" variant="ghost" size="icon" onClick={() => action.mutate(() => api.deleteModel(m.id))} aria-label={`Delete ${m.name}`}>
                      <Trash2 strokeWidth={iconStroke} />
                    </Button>
                  </>
                ) : (
                  <Button
                    variant={m.id === catalog.recommended ? 'default' : 'outline'}
                    size="sm"
                    disabled={downloading}
                    onClick={() => action.mutate(() => api.downloadModel(m.id))}
                  >
                    <ArrowDownToLine strokeWidth={iconStroke} /> Download
                  </Button>
                )}
              </div>
            </div>
          )
        })}
      </div>
      {!only && <EmbeddingModel catalog={catalog} downloading={downloading} busy={action.isPending} onDownload={() => action.mutate(() => api.downloadModel(catalog.embedding.id))} onDelete={() => action.mutate(() => api.deleteModel(catalog.embedding.id))} />}
      <p className="text-app-caption leading-relaxed text-muted-foreground">
        Models run on this computer with llama.cpp; nothing you ask leaves it. Downloads are 4-bit versions from Hugging
        Face, checked against their published checksum. That download is the only time LocalBoard goes online.
      </p>
    </div>
  )
}
