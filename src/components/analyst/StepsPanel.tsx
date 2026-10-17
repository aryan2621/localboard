import { useEffect, useState } from 'react'
import { Check, ChevronDown, Loader2, Square, X } from 'lucide-react'
import type { Step } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { useGeneration } from '../../stores/generation'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '../ui/collapsible'
import { Hint } from '../ui/tooltip'

function StepIcon({ status }: { status: Step['status'] }) {
  if (status === 'running') return <Loader2 size={14} strokeWidth={iconStroke} className="animate-spin text-brand" />
  if (status === 'done') return <Check size={14} strokeWidth={iconStroke} className="text-success" />
  if (status === 'error') return <X size={14} strokeWidth={iconStroke} className="text-destructive" />
  return <Square size={12} strokeWidth={iconStroke} className="text-muted-foreground" />
}

export function StepList({ steps }: { steps: Step[] }) {
  return (
    <ol>
      {steps.map((step, index) => (
        <li key={step.id} className="relative flex items-start gap-2.5 pb-2 text-app-label last:pb-0">
          {/* Connector line between step icons, so the steps read as one sequence. */}
          {index < steps.length - 1 && <span className="absolute bottom-0 left-[7px] top-5 w-px bg-border" aria-hidden />}
          <span className="relative mt-0.5 flex size-[14px] shrink-0 items-center justify-center rounded-full bg-background">
            <StepIcon status={step.status} />
          </span>
          <span className="min-w-0">
            <span className={step.status === 'running' ? 'text-foreground' : 'text-muted-foreground'}>{step.label}</span>
            {step.detail && <Hint text={step.detail}><span className="block truncate text-app-caption text-muted-foreground/80">{step.detail}</span></Hint>}
          </span>
        </li>
      ))}
    </ol>
  )
}

/** Whole seconds since `running` turned on. */
function useElapsed(running: boolean) {
  const [seconds, setSeconds] = useState(0)
  useEffect(() => {
    if (!running) return
    const started = Date.now()
    setSeconds(0)
    const timer = window.setInterval(() => setSeconds(Math.floor((Date.now() - started) / 1000)), 1000)
    return () => window.clearInterval(timer)
  }, [running])
  return seconds
}

/**
 * While work runs: one status line whose text is replaced as each step starts, with the full list
 * behind "Details". When finished: a folded summary that opens to the same list.
 */
export function StepsPanel({ steps, running, suffix }: { steps: Step[]; running?: boolean; suffix?: string }) {
  const [open, setOpen] = useState(false)
  const seconds = useElapsed(Boolean(running))
  if (steps.length === 0) return null
  const current = [...steps].reverse().find((step) => step.status === 'running') ?? steps[steps.length - 1]
  const finished = steps.filter((step) => step.status === 'done').length

  if (running) {
    const label = current.label.replace(/[.…]+$/, '')
    return (
      <div className="text-app-label">
        <div className="flex items-center gap-3" role="status" aria-live="polite">
          <span className="relative flex size-2.5 shrink-0" aria-hidden>
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-brand/40" />
            <span className="relative inline-flex size-2.5 rounded-full bg-brand" />
          </span>
          <span key={`${current.id}-${current.label}`} className="flex min-w-0 flex-1 animate-step-in items-baseline gap-2">
            <span className="shimmer-text shrink-0 font-medium">{label}…</span>
            {current.detail && <span className="truncate text-app-caption text-muted-foreground">{current.detail}</span>}
          </span>
          <span className="shrink-0 text-app-caption tabular-nums text-muted-foreground">
            {finished > 0 && `${finished} done · `}
            {seconds}s
          </span>
          <button
            type="button"
            onClick={() => setOpen(!open)}
            aria-expanded={open}
            className="flex shrink-0 items-center gap-0.5 rounded text-app-caption text-muted-foreground transition-colors duration-fast hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
          >
            Details
            <ChevronDown size={13} strokeWidth={iconStroke} className={open ? 'rotate-180 transition-transform' : 'transition-transform'} />
          </button>
        </div>
        {open && (
          <div className="mt-3 animate-rise-in pl-[22px]">
            <StepList steps={steps} />
          </div>
        )}
      </div>
    )
  }

  return (
    <Collapsible open={open} onOpenChange={setOpen} className="group text-app-label">
      <CollapsibleTrigger className="flex items-center gap-1 text-app-caption text-muted-foreground hover:text-foreground">
        <ChevronDown size={14} strokeWidth={iconStroke} className="-rotate-90 transition-transform group-data-[state=open]:rotate-0" />
        {steps.length} {steps.length === 1 ? 'step' : 'steps'}
        {suffix ?? ''}
      </CollapsibleTrigger>
      <CollapsibleContent className="mt-2 pl-5">
        <StepList steps={steps} />
      </CollapsibleContent>
    </Collapsible>
  )
}

/** The live steps of the dashboard, datasite, report, deck or automation being built. */
export function GenerationSteps({ running }: { running: boolean }) {
  const steps = useGeneration((state) => state.steps)
  if (!running) return null
  return <StepsPanel steps={steps} running />
}
