import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { ArrowLeft, ArrowRight, Check, Code2, Download, FileSpreadsheet, FolderOpen, KeyRound, LayoutDashboard, Loader2, Lock, MessageSquare, Shield } from 'lucide-react'
import { Banner, ChoiceGrid, IconTile } from '../../components/layout/patterns'
import { toast } from 'sonner'
import { ModelManager } from '../../components/models/ModelManager'
import { Button } from '../../components/ui/button'
import { api, errorMessage } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { pickSources, useAddSources, usePendingSources } from '../../lib/sources'
import { cn } from '../../lib/utils'

const STEPS = ['welcome', 'model', 'privacy', 'data'] as const
type Step = (typeof STEPS)[number]

const STEP_INFO: Record<Step, { label: string; hint: string }> = {
  welcome: { label: 'Welcome', hint: 'What LocalBoard does' },
  model: { label: 'AI model', hint: 'Download one to this computer' },
  privacy: { label: 'Privacy', hint: 'Where your data goes' },
  data: { label: 'Your data', hint: 'Add files or a folder' },
}

const POINTS = [
  { icon: MessageSquare, title: 'Ask in plain words', body: 'Questions about your spreadsheets, files and databases, answered with tables and charts.' },
  { icon: Code2, title: 'Check every answer', body: 'Each answer shows the SQL it ran, so you can see exactly how it was worked out.' },
  { icon: LayoutDashboard, title: 'Share the results', body: 'Turn answers into dashboards, reports, decks and scheduled checks.' },
]

const PROMISES = [
  { icon: Lock, title: 'Nothing leaves this computer', body: 'Your data, questions and results are never sent anywhere. No account, and no usage data is collected.' },
  { icon: Download, title: 'Internet only for models', body: 'LocalBoard connects only to download a model, and only to Hugging Face for that file.' },
  { icon: KeyRound, title: 'Read-only and keychain', body: 'Databases are opened read-only. Saved passwords stay in your system keychain.' },
]

function Heading({ title, description }: { title: string; description?: string }) {
  return (
    <div className="space-y-2">
      <h2 className="font-serif text-app-page text-foreground">{title}</h2>
      {description && <p className="max-w-lg text-app-label leading-relaxed text-muted-foreground">{description}</p>}
    </div>
  )
}

function FeatureList({ items }: { items: typeof POINTS }) {
  return (
    <div className="grid gap-3">
      {items.map(({ icon, title, body }) => (
        <div key={title} className="flex gap-4 rounded-xl border border-border bg-surface p-4">
          <IconTile icon={icon} />
          <div className="space-y-0.5">
            <p className="text-app-label font-semibold text-foreground">{title}</p>
            <p className="text-app-label leading-relaxed text-muted-foreground">{body}</p>
          </div>
        </div>
      ))}
    </div>
  )
}

function Welcome() {
  return (
    <div className="space-y-6">
      <Heading title="A private data analyst on your desktop" description="Set up takes a few minutes. You can change everything later in Settings." />
      <FeatureList items={POINTS} />
    </div>
  )
}

function ModelStep() {
  return (
    <div className="space-y-6">
      <Heading title="Choose an AI model" description="This is the one we recommend for this computer. You can switch models or add more later in Settings." />
      <ModelManager only="recommended" />
    </div>
  )
}

function Privacy() {
  return (
    <div className="space-y-6">
      <Heading title="Your data stays here" description="LocalBoard runs the AI model and the database engine on this computer." />
      <FeatureList items={PROMISES} />
    </div>
  )
}

type DataChoice = 'files' | 'folder'

function DataStep({ added, onAdded }: { added: number; onAdded: (tables: number) => void }) {
  const add = useAddSources()
  const pending = usePendingSources()
  const adding = pending.length > 0
  const choose = async (choice: DataChoice) => {
    const directory = choice === 'folder'
    const paths = await pickSources(directory)
    if (paths.length > 0) add.mutate({ paths, recursive: directory }, { onSuccess: ({ tables }) => onAdded(tables) })
  }

  return (
    <div className="space-y-6">
      <Heading title="Add your first data" description="CSV, TSV, Excel, Parquet or JSON. You can connect databases from Connections after setup." />
      <ChoiceGrid<DataChoice>
        columns={2}
        onSelect={(choice) => void choose(choice)}
        options={[
          { value: 'files', icon: FileSpreadsheet, title: 'Choose files', description: 'Pick one or more files. Each becomes a table.', disabled: adding },
          { value: 'folder', icon: FolderOpen, title: 'Watch a folder', description: 'Every file inside is added and kept in sync as it changes.', disabled: adding },
        ]}
      />
      {adding && (
        <Banner
          icon={Loader2}
          title={`Reading ${pending.length === 1 ? '1 source' : `${pending.length} sources`}`}
          description="Profiling columns so the analyst knows what is in each table."
        />
      )}
      {added > 0 && (
        <Banner
          icon={Check}
          tone="success"
          title={`${added} ${added === 1 ? 'table is' : 'tables are'} ready`}
          description="Add more, or start exploring."
        />
      )}
    </div>
  )
}

export function Onboarding() {
  const [step, setStep] = useState<Step>('welcome')
  const [tablesAdded, setTablesAdded] = useState(0)
  const { data: catalog } = useQuery({ queryKey: queryKeys.models, queryFn: api.modelCatalog })
  const index = STEPS.indexOf(step)
  const modelReady = Boolean(catalog?.models.some((model) => model.installed))

  const finish = useMutation({
    mutationFn: api.completeOnboarding,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.status }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  const next = () => (index === STEPS.length - 1 ? finish.mutate() : setStep(STEPS[index + 1]))

  return (
    <div className="flex h-screen bg-background">
      <aside className="flex w-80 shrink-0 flex-col border-r border-border bg-sidebar">
        <div className="mac-window-space mac-drag-region shrink-0" />
        <div className="flex items-center gap-3 px-8 pb-10 pt-6">
          <img src="/app-icon.png" alt="" className="h-10 w-10 rounded-lg" />
          <div>
            <p className="font-serif text-app-title text-foreground">LocalBoard</p>
            <p className="text-app-caption text-muted-foreground">Set up</p>
          </div>
        </div>
        <ol className="space-y-1 px-5" aria-label="Setup steps">
          {STEPS.map((item, position) => {
            const done = position < index
            const active = position === index
            return (
              <li key={item} className="relative">
                {position < STEPS.length - 1 && <span className={cn('absolute left-[23px] top-11 h-[calc(100%-1.75rem)] w-px', done ? 'bg-brand' : 'bg-border')} aria-hidden />}
                <button
                  type="button"
                  disabled={position > index}
                  onClick={() => setStep(item)}
                  aria-current={active ? 'step' : undefined}
                  className={cn('flex w-full items-start gap-3 rounded-lg px-3 py-2.5 text-left transition-colors duration-fast', active ? 'bg-accent' : 'enabled:hover:bg-accent/50')}
                >
                  <span
                    className={cn(
                      'flex size-6 shrink-0 items-center justify-center rounded-full border text-app-caption font-semibold tabular-nums',
                      done ? 'border-brand bg-brand text-brand-foreground' : active ? 'border-brand bg-background text-brand' : 'border-border bg-background text-muted-foreground',
                    )}
                  >
                    {done ? <Check size={13} strokeWidth={2.5} /> : position + 1}
                  </span>
                  <span className="min-w-0">
                    <span className={cn('block text-app-label', active ? 'font-semibold text-foreground' : 'text-foreground/80')}>{STEP_INFO[item].label}</span>
                    <span className="block text-app-caption text-muted-foreground">{STEP_INFO[item].hint}</span>
                  </span>
                </button>
              </li>
            )
          })}
        </ol>
        <div className="mt-auto flex gap-2 px-8 py-6 text-app-caption text-muted-foreground">
          <Shield size={14} strokeWidth={iconStroke} className="mt-0.5 shrink-0" />
          Everything runs on this computer.
        </div>
      </aside>
      <main className="flex min-w-0 flex-1 flex-col">
        <div className="mac-window-space mac-drag-region shrink-0" />
        <div className="min-h-0 flex-1 overflow-y-auto">
          <div key={step} className="mx-auto w-full max-w-2xl animate-rise-in px-10 py-12">
            <p className="mb-3 text-app-caption font-medium text-brand">
              Step {index + 1} of {STEPS.length}
            </p>
            {step === 'welcome' && <Welcome />}
            {step === 'model' && <ModelStep />}
            {step === 'privacy' && <Privacy />}
            {step === 'data' && <DataStep added={tablesAdded} onAdded={(tables) => setTablesAdded((n) => n + tables)} />}
          </div>
        </div>
        <footer className="flex items-center justify-between gap-3 border-t border-border px-10 py-4">
          <div>
            {index > 0 && (
              <Button variant="ghost" onClick={() => setStep(STEPS[index - 1])}>
                <ArrowLeft strokeWidth={iconStroke} /> Back
              </Button>
            )}
          </div>
          <div className="flex items-center gap-2">
            {step === 'model' && !modelReady && (
              <Button variant="ghost" onClick={next}>
                Continue in SQL-only mode
              </Button>
            )}
            {step === 'data' && tablesAdded === 0 && (
              <Button variant="ghost" disabled={finish.isPending} onClick={next}>
                Skip for now
              </Button>
            )}
            {!(step === 'model' && !modelReady) && !(step === 'data' && tablesAdded === 0) && (
              <Button loading={finish.isPending} onClick={next}>
                {step === 'data' ? 'Start exploring' : step === 'welcome' ? 'Get started' : 'Continue'}
                {step !== 'data' && <ArrowRight strokeWidth={iconStroke} />}
              </Button>
            )}
          </div>
        </footer>
      </main>
    </div>
  )
}
