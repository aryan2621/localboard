import { useState, type ReactNode } from 'react'
import { Check, Loader2, PanelLeftClose, PanelLeftOpen, type LucideIcon } from 'lucide-react'
import { cn } from '../../lib/utils'
import { iconStroke } from '../../lib/icon'
import { Button } from '../ui/button'
import { Hint } from '../ui/tooltip'

/* Shared layout patterns: panes, stat rows, wizards, timelines and choice cards. */

const TONES = {
  brand: 'bg-brand/10 text-brand',
  success: 'bg-success/10 text-success',
  warning: 'bg-warning/10 text-warning',
  destructive: 'bg-destructive/10 text-destructive',
  muted: 'bg-muted text-muted-foreground',
} as const
export type Tone = keyof typeof TONES

/** Tinted rounded square holding an icon; the visual anchor of cards and rows. */
export function IconTile({ icon: Icon, tone = 'brand', size = 'md', className }: { icon: LucideIcon; tone?: Tone; size?: 'sm' | 'md' | 'lg'; className?: string }) {
  const box = size === 'sm' ? 'size-7 rounded-md' : size === 'lg' ? 'size-12 rounded-xl' : 'size-9 rounded-lg'
  const glyph = size === 'sm' ? 14 : size === 'lg' ? 22 : 18
  return (
    <span className={cn('flex shrink-0 items-center justify-center', box, TONES[tone], className)}>
      <Icon size={glyph} strokeWidth={iconStroke} />
    </span>
  )
}

const AVATAR_TONES = ['bg-chart-1/15 text-chart-1', 'bg-chart-2/15 text-chart-2', 'bg-chart-3/15 text-chart-3', 'bg-chart-5/15 text-chart-5', 'bg-chart-6/15 text-chart-6']

/** Initials in a tinted circle; the colour is stable for a given name. */
export function Avatar({ name, size = 'md', className }: { name: string; size?: 'sm' | 'md' | 'lg'; className?: string }) {
  const initials =
    name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0]?.toUpperCase())
      .join('') || '?'
  const tone = AVATAR_TONES[[...name].reduce((sum, ch) => sum + ch.charCodeAt(0), 0) % AVATAR_TONES.length]
  const box = size === 'sm' ? 'size-8 text-app-caption' : size === 'lg' ? 'size-14 text-app-title' : 'size-10 text-app-label'
  return <span className={cn('flex shrink-0 items-center justify-center rounded-full font-semibold', box, tone, className)}>{initials}</span>
}

export type Stat = { label: string; value: ReactNode; hint?: ReactNode; icon?: LucideIcon; tone?: Tone; onClick?: () => void }

/** A row of headline numbers separated by hairlines. */
const STAT_COLUMNS: Record<number, string> = { 1: 'grid-cols-1', 2: 'sm:grid-cols-2', 3: 'sm:grid-cols-3', 4: 'lg:grid-cols-4' }

/** A row of separate number cards. */
export function StatRow({ stats, className }: { stats: Stat[]; className?: string }) {
  return (
    <div className={cn('grid grid-cols-2 gap-3', STAT_COLUMNS[Math.min(stats.length, 4)], className)}>
      {stats.map((stat) => {
        const body = (
          <>
            <span className="flex items-center gap-1.5 text-app-caption text-muted-foreground">
              {stat.icon && <stat.icon size={13} strokeWidth={iconStroke} className={stat.tone ? TONES[stat.tone].split(' ')[1] : undefined} />}
              {stat.label}
            </span>
            <span className="mt-1 block font-serif text-app-heading tabular-nums text-foreground">{stat.value}</span>
            {stat.hint && <span className="mt-0.5 block truncate text-app-caption text-muted-foreground">{stat.hint}</span>}
          </>
        )
        return stat.onClick ? (
          <button
            key={stat.label}
            type="button"
            onClick={stat.onClick}
            className="min-w-0 rounded-xl border border-border bg-surface px-4 py-3 text-left transition-colors duration-fast hover:border-foreground/25 hover:bg-accent/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
          >
            {body}
          </button>
        ) : (
          <div key={stat.label} className="min-w-0 rounded-xl border border-border bg-surface px-4 py-3">
            {body}
          </div>
        )
      })}
    </div>
  )
}

/**
 * Master-detail inside the page: a list or tools pane on the left and the main content on the right.
 * The app sidebar stays as it is. With `collapsible`, the pane folds to a thin strip and remembers that.
 */
export function SplitView({
  list,
  detail,
  listWidth = 300,
  className,
  collapsible = false,
  storageKey,
  label = 'panel',
}: {
  list: ReactNode
  detail: ReactNode
  listWidth?: number
  className?: string
  collapsible?: boolean
  storageKey?: string
  label?: string
}) {
  const key = storageKey ? `split.${storageKey}.collapsed` : null
  const [collapsed, setCollapsed] = useState(() => {
    if (!collapsible || !key) return false
    try {
      return window.localStorage.getItem(key) === '1'
    } catch {
      return false
    }
  })
  const toggle = () => {
    const next = !collapsed
    setCollapsed(next)
    if (!key) return
    try {
      window.localStorage.setItem(key, next ? '1' : '0')
    } catch {
      // Remembering is a convenience only.
    }
  }

  return (
    <div className={cn('flex min-h-0 flex-1', className)}>
      {collapsed ? (
        <aside className="flex w-11 shrink-0 flex-col items-center border-r border-border bg-sidebar/40 pt-4">
          <Hint text={`Show ${label}`}>
            <Button variant="ghost" size="icon" aria-label={`Show ${label}`} onClick={toggle}>
              <PanelLeftOpen strokeWidth={iconStroke} />
            </Button>
          </Hint>
        </aside>
      ) : (
        <aside className="flex min-h-0 shrink-0 flex-col border-r border-border bg-sidebar/40" style={{ width: listWidth }}>
          <div className="flex min-h-0 flex-1 flex-col">{list}</div>
          {collapsible && (
            <button
              type="button"
              onClick={toggle}
              className="flex items-center gap-2 border-t border-border px-4 py-2.5 text-app-caption text-muted-foreground transition-colors duration-fast hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/40"
            >
              <PanelLeftClose size={15} strokeWidth={iconStroke} /> Hide {label}
            </button>
          )}
        </aside>
      )}
      <section className="min-h-0 min-w-0 flex-1 overflow-y-auto">{detail}</section>
    </div>
  )
}

/** Header strip of a list pane: title, count and an action. */
export function PaneHeader({ title, count, action }: { title: string; count?: number; action?: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-2 px-4 pb-2 pt-5">
      <h1 className="font-serif text-app-title text-foreground">
        {title}
        {count !== undefined && <span className="ml-1.5 font-sans text-app-caption text-muted-foreground">{count}</span>}
      </h1>
      {action}
    </div>
  )
}

/** A selectable row in a list pane. */
export function PaneItem({
  selected,
  onClick,
  leading,
  title,
  meta,
  trailing,
}: {
  selected?: boolean
  onClick: () => void
  leading?: ReactNode
  title: ReactNode
  meta?: ReactNode
  trailing?: ReactNode
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={selected ? 'true' : undefined}
      className={cn(
        'flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40',
        selected ? 'bg-accent' : 'hover:bg-accent/50',
      )}
    >
      {leading}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-app-label font-medium text-foreground">{title}</span>
        {meta && <span className="mt-0.5 block truncate text-app-caption text-muted-foreground">{meta}</span>}
      </span>
      {trailing}
    </button>
  )
}

/** Large selectable cards in a grid: pick a type, a connector, a step. */
export function ChoiceGrid<T extends string>({
  options,
  value,
  onSelect,
  columns = 3,
}: {
  options: { value: T; title: string; description?: string; icon: LucideIcon; tone?: Tone; badge?: ReactNode; disabled?: boolean }[]
  value?: T | null
  onSelect: (value: T) => void
  columns?: 2 | 3 | 4
}) {
  const grid = columns === 2 ? 'sm:grid-cols-2' : columns === 4 ? 'sm:grid-cols-2 lg:grid-cols-4' : 'sm:grid-cols-3'
  return (
    <div className={cn('grid gap-3', grid)}>
      {options.map((option) => {
        const active = value === option.value
        return (
          <button
            key={option.value}
            type="button"
            disabled={option.disabled}
            onClick={() => onSelect(option.value)}
            aria-pressed={active}
            className={cn(
              'flex flex-col items-start gap-2 rounded-xl border bg-surface p-4 text-left transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40 disabled:cursor-not-allowed disabled:opacity-50',
              active ? 'border-brand ring-1 ring-brand/40' : 'border-border hover:border-foreground/25 hover:bg-accent/30',
            )}
          >
            <span className="flex w-full items-start justify-between gap-2">
              <IconTile icon={option.icon} tone={option.tone} />
              {option.badge}
            </span>
            <span className="text-app-label font-semibold text-foreground">{option.title}</span>
            {option.description && <span className="text-app-caption text-muted-foreground">{option.description}</span>}
          </button>
        )
      })}
    </div>
  )
}

/** Numbered wizard steps: done, current, upcoming. */
export function Stepper({ steps, current }: { steps: string[]; current: number }) {
  return (
    <ol className="flex items-center gap-2" aria-label="Progress">
      {steps.map((label, index) => {
        const done = index < current
        const active = index === current
        return (
          <li key={label} className="flex min-w-0 items-center gap-2">
            <span
              className={cn(
                'flex size-6 shrink-0 items-center justify-center rounded-full border text-app-caption font-semibold tabular-nums',
                done ? 'border-brand bg-brand text-brand-foreground' : active ? 'border-brand text-brand' : 'border-border text-muted-foreground',
              )}
              aria-current={active ? 'step' : undefined}
            >
              {done ? <Check size={13} strokeWidth={2.5} /> : index + 1}
            </span>
            <span className={cn('truncate text-app-caption', active ? 'font-medium text-foreground' : 'text-muted-foreground')}>{label}</span>
            {index < steps.length - 1 && <span className="h-px w-6 shrink-0 bg-border" aria-hidden />}
          </li>
        )
      })}
    </ol>
  )
}

export type TimelineItem = { id: string; icon: LucideIcon; title: ReactNode; meta?: ReactNode; status?: 'done' | 'running' | 'error' | 'idle'; actions?: ReactNode; children?: ReactNode }

/** Vertical connected list: pipeline steps, run history, progress. */
export function Timeline({ items }: { items: TimelineItem[] }) {
  return (
    <ol className="relative">
      {items.map((item, index) => {
        const tone: Tone = item.status === 'error' ? 'destructive' : item.status === 'done' ? 'success' : item.status === 'running' ? 'brand' : 'muted'
        return (
          <li key={item.id} className="relative flex gap-3 pb-5 last:pb-0">
            {index < items.length - 1 && <span className="absolute left-[17px] top-10 h-[calc(100%-2.5rem)] w-px bg-border" aria-hidden />}
            {item.status === 'running' ? (
              <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-brand/10 text-brand">
                <Loader2 size={18} strokeWidth={iconStroke} className="animate-spin" />
              </span>
            ) : (
              <IconTile icon={item.icon} tone={tone} />
            )}
            <div className="min-w-0 flex-1 rounded-xl border border-border bg-surface px-4 py-3">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-app-label font-medium text-foreground">{item.title}</div>
                  {item.meta && <div className="mt-0.5 text-app-caption text-muted-foreground">{item.meta}</div>}
                </div>
                {item.actions && <div className="flex shrink-0 items-center gap-1">{item.actions}</div>}
              </div>
              {item.children && <div className="mt-3">{item.children}</div>}
            </div>
          </li>
        )
      })}
    </ol>
  )
}

/** Thin progress bar with an optional label. */
export function Meter({ value, label, tone = 'brand', className }: { value: number; label?: ReactNode; tone?: 'brand' | 'success' | 'warning' | 'destructive'; className?: string }) {
  const pct = Math.max(0, Math.min(100, value))
  const bar = { brand: 'bg-brand', success: 'bg-success', warning: 'bg-warning', destructive: 'bg-destructive' }[tone]
  return (
    <div className={cn('flex items-center gap-2', className)}>
      <div className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-muted" role="meter" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
        <div className={cn('h-full rounded-full transition-[width] duration-base', bar)} style={{ width: `${pct}%` }} />
      </div>
      {label !== undefined && <span className="shrink-0 text-app-caption tabular-nums text-muted-foreground">{label}</span>}
    </div>
  )
}

export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return <kbd className={cn('rounded border border-border bg-muted px-1.5 py-px font-sans text-app-caption text-muted-foreground', className)}>{children}</kbd>
}

/** Soft full-width notice with actions. */
export function Banner({ icon: Icon, tone = 'brand', title, description, actions, className }: { icon: LucideIcon; tone?: Tone; title: ReactNode; description?: ReactNode; actions?: ReactNode; className?: string }) {
  return (
    <div className={cn('flex flex-wrap items-center gap-3 rounded-xl border border-border bg-surface px-4 py-3', className)}>
      <IconTile icon={Icon} tone={tone} size="sm" />
      <div className="min-w-0 flex-1">
        <p className="text-app-label font-medium text-foreground">{title}</p>
        {description && <p className="text-app-caption text-muted-foreground">{description}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </div>
  )
}

/** Appears only when there are unsaved edits; sticks to the bottom of the scrolling pane. */
export function SaveBar({ dirty, saving, onSave, onDiscard, label = 'Unsaved changes' }: { dirty: boolean; saving?: boolean; onSave: () => void; onDiscard: () => void; label?: string }) {
  if (!dirty && !saving) return null
  return (
    <div className="sticky bottom-4 z-10 mx-auto flex w-full max-w-xl animate-rise-in items-center gap-3 rounded-xl border border-border bg-popover px-4 py-2.5 shadow-lg">
      <span className="size-2 rounded-full bg-warning" aria-hidden />
      <span className="flex-1 text-app-label text-foreground">{label}</span>
      <Button variant="ghost" size="sm" disabled={saving} onClick={onDiscard}>
        Discard
      </Button>
      <Button size="sm" loading={saving} onClick={onSave}>
        Save
      </Button>
    </div>
  )
}

/** A titled card that groups related fields. */
export function Panel({ title, description, actions, children, className }: { title: string; description?: string; actions?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cn('rounded-xl border border-border bg-surface', className)}>
      <header className="flex items-start justify-between gap-3 border-b border-border px-5 py-3.5">
        <div className="min-w-0">
          <h3 className="text-app-label font-semibold text-foreground">{title}</h3>
          {description && <p className="mt-0.5 text-app-caption text-muted-foreground">{description}</p>}
        </div>
        {actions}
      </header>
      <div className="p-5">{children}</div>
    </section>
  )
}

/** Small selectable pills with "Clear" — quick filters above a list or table. */
export function FilterChips<T extends string>({ options, value, onChange, allLabel = 'All' }: { options: { value: T; label: string; count?: number }[]; value: T | null; onChange: (value: T | null) => void; allLabel?: string }) {
  const chip = (active: boolean) =>
    cn(
      'inline-flex h-9 items-center gap-1.5 rounded-lg border px-3.5 text-app-label font-medium transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40',
      active ? 'border-brand/50 bg-brand/10 text-foreground' : 'border-border bg-surface text-muted-foreground hover:text-foreground',
    )
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <button type="button" className={chip(value === null)} onClick={() => onChange(null)} aria-pressed={value === null}>
        {allLabel}
      </button>
      {options.map((option) => (
        <button key={option.value} type="button" className={chip(value === option.value)} onClick={() => onChange(value === option.value ? null : option.value)} aria-pressed={value === option.value}>
          {option.label}
          {option.count !== undefined && <span className="tabular-nums text-muted-foreground">{option.count}</span>}
        </button>
      ))}
    </div>
  )
}
