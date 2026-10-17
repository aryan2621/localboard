import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { ArrowUp, Loader2 } from 'lucide-react'
import { api } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { queryKeys } from '../../lib/queryClient'
import { Button } from '../ui/button'
import { cn } from '../../lib/utils'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select'

const MAX_QUESTION_CHARS = 20_000

/** Mirrors the sidebar status: without a usable model, questions must be SQL. */
export function useAiReady() {
  const { data: catalog } = useQuery({ queryKey: queryKeys.models, queryFn: api.modelCatalog })
  return Boolean(catalog?.runtimeAvailable && catalog.active)
}

/** The untyped tail of a past question or schema term, shown faintly until Tab accepts it. */
function suggestionFor(draft: string, questions: string[], terms: string[]) {
  if (!draft || draft !== draft.trimStart() || draft.includes('\n') || /^\s*(select|with)\b/i.test(draft)) return ''
  const question = questions.find((item) => item.toLowerCase().startsWith(draft.toLowerCase()) && item.length > draft.length)
  if (question) return question.slice(draft.length)
  const word = draft.match(/[A-Za-z0-9]{2,}$/)?.[0]
  if (!word) return ''
  const term = terms.find((item) => item.toLowerCase().startsWith(word.toLowerCase()) && item.length > word.length)
  return term ? term.slice(word.length) : ''
}

export type OutputKind = 'answer' | 'dashboard' | 'datasite' | 'report' | 'deck' | 'automation'

const OUTPUT_CHIPS: { value: OutputKind; label: string }[] = [
  { value: 'answer', label: 'Answer' },
  { value: 'dashboard', label: 'Dashboard' },
  { value: 'datasite', label: 'Datasite' },
  { value: 'report', label: 'PDF report' },
  { value: 'deck', label: 'Presentation' },
  { value: 'automation', label: 'Automation' },
]

export function Composer({
  onSubmit,
  busy,
  disabled,
  autoFocus,
  draftValue,
  onDraftChange,
  chips,
  outputValue,
}: {
  onSubmit: (question: string, output: OutputKind) => Promise<unknown>
  busy: boolean
  disabled?: boolean
  autoFocus?: boolean
  draftValue?: string
  onDraftChange?: (value: string) => void
  /** Shows the output choices as a row of chips under the box instead of a dropdown. */
  chips?: boolean
  /** Lets the page preset the output, e.g. from a "Start from" card. */
  outputValue?: OutputKind
}) {
  const [draft, setDraft] = useState(draftValue ?? '')

  useEffect(() => {
    if (draftValue !== undefined) {
      setDraft(draftValue)
    }
  }, [draftValue])

  const setNextDraft = (next: string) => {
    setDraft(next)
    onDraftChange?.(next)
  }

  const [output, setOutput] = useState<OutputKind>('answer')
  useEffect(() => {
    if (outputValue) setOutput(outputValue)
  }, [outputValue])
  const aiReady = useAiReady()
  const { data: sources } = useQuery({ queryKey: queryKeys.completions, queryFn: api.completionSources })
  const ghost = suggestionFor(draft, sources?.questions ?? [], sources?.terms ?? [])
  const question = draft.trim()
  const blocked = disabled || busy || question.length === 0

  const submit = () => {
    if (blocked) return
    setNextDraft('')
    onSubmit(question, output).then(
      () => undefined,
      () => setNextDraft(question),
    )
  }

  return (
    <div className="space-y-3">
    <form
      className={cn(
        'flex items-end gap-2 rounded-lg border border-input bg-surface p-1.5 shadow-sm transition-shadow focus-within:ring-2 focus-within:ring-ring/40',
      )}
      onSubmit={(event) => {
        event.preventDefault()
        submit()
      }}
    >
      {!chips && (
      <Select value={output} disabled={busy} onValueChange={(value) => setOutput(value as OutputKind)}>
        <SelectTrigger aria-label={'Choose an output'} className="w-auto max-w-[9.5rem] shrink-0 border-transparent bg-transparent px-2 text-app-caption hover:bg-accent">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="answer">{'Answer'}</SelectItem>
          <SelectItem value="dashboard" disabled={!aiReady}>{'Dashboard'}</SelectItem>
          <SelectItem value="datasite" disabled={!aiReady}>{'Datasite'}</SelectItem>
          <SelectItem value="report" disabled={!aiReady}>{'PDF report'}</SelectItem>
          <SelectItem value="deck" disabled={!aiReady}>{'Presentation'}</SelectItem>
          <SelectItem value="app" disabled={!aiReady}>{'Data app'}</SelectItem>
          <SelectItem value="automation" disabled={!aiReady}>{'Automation'}</SelectItem>
        </SelectContent>
      </Select>
      )}
      <div className="relative min-h-8 flex-1">
        <div aria-hidden className="pointer-events-none absolute inset-0 whitespace-pre-wrap break-words px-2 py-1.5 text-app-label leading-5">
          <span className="text-transparent">{draft}</span>
          {ghost && <span className="text-muted-foreground/45">{ghost}</span>}
        </div>
        <textarea
          aria-label={'Question'}
          aria-keyshortcuts={ghost ? 'Tab' : undefined}
          autoFocus={autoFocus}
          disabled={busy || disabled}
          value={draft}
          maxLength={MAX_QUESTION_CHARS}
          rows={Math.min(8, Math.max(1, draft.split('\n').length))}
          placeholder={aiReady ? 'Ask a question, or paste a SELECT query…' : 'SQL-only mode: write a SELECT query…'}
          onChange={(event) => setNextDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Tab' && ghost && !event.shiftKey) {
              event.preventDefault()
              setNextDraft(draft + ghost)
              return
            }
            if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault()
              submit()
            }
          }}
          className="relative max-h-60 w-full resize-none disabled:cursor-not-allowed disabled:opacity-60 whitespace-pre-wrap break-words bg-transparent px-2 py-1.5 text-app-label leading-5 text-foreground placeholder:text-muted-foreground focus:outline-none"
        />
        {ghost && <span className="sr-only">{`Suggestion: ${draft + ghost}. Press Tab to accept.`}</span>}
      </div>
      <Button type="submit" size="icon" className="h-8 w-8 shrink-0 rounded-md" disabled={blocked} aria-label={'Ask'}>
        {busy ? <Loader2 strokeWidth={iconStroke} className="animate-spin" /> : <ArrowUp strokeWidth={iconStroke} />}
      </Button>
    </form>
    {chips && (
      <div className="flex flex-wrap justify-center gap-2" role="radiogroup" aria-label="Choose an output">
        {OUTPUT_CHIPS.map((chip) => {
          const active = output === chip.value
          const unavailable = chip.value !== 'answer' && !aiReady
          return (
            <button
              key={chip.value}
              type="button"
              role="radio"
              aria-checked={active}
              disabled={busy || unavailable}
              onClick={() => setOutput(chip.value)}
              className={cn(
                'rounded-full border px-3 py-1 text-app-caption font-medium transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40 disabled:cursor-not-allowed disabled:opacity-50',
                active ? 'border-brand/50 bg-brand/10 text-foreground' : 'border-border bg-surface text-muted-foreground hover:text-foreground',
              )}
            >
              {chip.label}
            </button>
          )
        })}
      </div>
    )}
    </div>
  )
}
