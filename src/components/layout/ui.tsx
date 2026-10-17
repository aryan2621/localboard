import { useState, type ReactNode } from 'react'
import { ArrowLeft, Check, ChevronDown, ChevronRight, Copy, LayoutGrid, List, Loader2, MoreHorizontal, Search, type LucideIcon } from 'lucide-react'
import { cn } from '../../lib/utils'
import { iconStroke } from '../../lib/icon'
import { Badge } from '../ui/badge'
import { Button } from '../ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from '../ui/dropdown-menu'
import { Input } from '../ui/input'
import { Label } from '../ui/label'
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '../ui/alert-dialog'
import { Hint } from '../ui/tooltip'
import { Skeleton } from '../ui/skeleton'

export function SettingsSection({
  title,
  description,
  children,
  className,
  collapsible,
  defaultOpen = false,
}: {
  title?: string
  description?: string
  children: ReactNode
  className?: string
  /** Rarely-changed groups fold away so the page shows only what matters first. */
  collapsible?: boolean
  defaultOpen?: boolean
}) {
  if (collapsible && title) {
    return (
      <details open={defaultOpen} className={cn('group rounded-xl border border-border bg-surface/40', className)}>
        <summary className="flex cursor-pointer list-none items-center justify-between gap-3 px-4 py-3 [&::-webkit-details-marker]:hidden">
          <span className="min-w-0">
            <span className="block text-app-label font-semibold text-foreground">{title}</span>
            {description && <span className="mt-0.5 block truncate text-app-caption text-muted-foreground">{description}</span>}
          </span>
          <ChevronDown size={16} className="shrink-0 text-muted-foreground transition-transform group-open:rotate-180" />
        </summary>
        <div className="space-y-4 border-t border-border p-4">{children}</div>
      </details>
    )
  }
  return (
    <section className={cn('space-y-3', className)}>
      {(title || description) && (
        <div className="space-y-1">
          {title && <h3 className="text-app-label font-semibold text-foreground">{title}</h3>}
          {description && <p className="text-app-caption text-muted-foreground leading-relaxed">{description}</p>}
        </div>
      )}
      {children}
    </section>
  )
}

/** A card row holding only a button, right-aligned, instead of a row with a redundant label. */
export function SettingsAction({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn('flex flex-wrap items-center justify-end gap-2 px-4 py-3', className)}>{children}</div>
}

export function SettingsCard({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn('rounded-xl border border-border bg-surface divide-y divide-border', className)}>{children}</div>
}

export function SettingsRow({
  label,
  description,
  children,
  className,
}: {
  label: string
  description?: string
  children: ReactNode
  className?: string
}) {
  return (
    <div className={cn('flex items-center justify-between gap-4 px-4 py-3.5 first:rounded-t-xl last:rounded-b-xl', className)}>
      <div className="min-w-0 flex-1">
        <p className="text-app-label text-foreground">{label}</p>
        {description && <p className="text-app-caption text-muted-foreground mt-0.5 leading-relaxed">{description}</p>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  )
}

export function SegmentedControl<T extends string>({
  value,
  options,
  onChange,
  className,
}: {
  value: T
  options: { value: T; label: string }[]
  onChange: (value: T) => void
  className?: string
}) {
  return (
    <div className={cn('inline-flex rounded-lg border border-border bg-muted/50 p-0.5', className)} role="group">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
          className={cn(
            'px-3 h-7 rounded-md text-app-label font-medium transition-colors duration-fast',
            value === option.value ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground',
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  )
}

export function StatusBadge({
  variant,
  children,
}: {
  variant: 'success' | 'warning' | 'error' | 'neutral' | 'brand'
  children: ReactNode
}) {
  const map = { success: 'success', warning: 'warning', error: 'destructive', neutral: 'secondary', brand: 'brand' } as const
  return <Badge variant={map[variant]}>{children}</Badge>
}

/* ---------- Page system: one container, one header, one set of states ---------- */

const PAGE_WIDTH = { narrow: 'max-w-3xl', wide: 'max-w-6xl', full: 'max-w-[1400px]' } as const

export function PageContainer({
  width = 'narrow',
  className,
  children,
}: {
  width?: keyof typeof PAGE_WIDTH
  className?: string
  children: ReactNode
}) {
  return <div className={cn('mx-auto space-y-8 px-8 py-10 md:px-12', PAGE_WIDTH[width], className)}>{children}</div>
}

export function PageHeader({
  title,
  description,
  actions,
  className,
}: {
  title: string
  description?: string
  actions?: ReactNode
  className?: string
}) {
  return (
    <header className={cn('flex flex-wrap items-end justify-between gap-4', className)}>
      <div className="min-w-0 space-y-1">
        <h1 className="font-serif text-app-page text-foreground">{title}</h1>
        {description && <p className="text-app-body text-muted-foreground">{description}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  )
}

/** Detail view header: back, title, up to three visible actions and a More menu for the rest. */
export function DetailHeader({
  title,
  onBack,
  actions,
  menu,
  children,
}: {
  title: ReactNode
  onBack: () => void
  actions?: ReactNode
  /** DropdownMenuItem children. The More button is omitted when absent. */
  menu?: ReactNode
  children?: ReactNode
}) {
  return (
    <div className="sticky top-0 z-10 flex flex-wrap items-center gap-2 border-b border-border bg-background/85 px-6 py-3 backdrop-blur">
      <Hint text="Back">
        <Button variant="ghost" size="icon" aria-label="Back" onClick={onBack}>
          <ArrowLeft strokeWidth={iconStroke} />
        </Button>
      </Hint>
      <div className="min-w-0 flex-1">{typeof title === 'string' ? <h1 className="truncate font-serif text-app-title text-foreground">{title}</h1> : title}</div>
      {children}
      {actions}
      {menu && <MoreMenu>{menu}</MoreMenu>}
    </div>
  )
}

export function MoreMenu({ children, label = 'More actions' }: { children: ReactNode; label?: string }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="icon" aria-label={label}>
          <MoreHorizontal strokeWidth={iconStroke} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">{children}</DropdownMenuContent>
    </DropdownMenu>
  )
}

/** Quiet overflow menu for list rows; stays hidden until the row is hovered or focused. */
export function RowMenu({ children, label = 'More actions', className }: { children: ReactNode; label?: string; className?: string }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          aria-label={label}
          className={cn('h-8 w-8 opacity-0 transition-opacity duration-fast focus-visible:opacity-100 group-hover:opacity-100 data-[state=open]:opacity-100', className)}
        >
          <MoreHorizontal strokeWidth={iconStroke} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">{children}</DropdownMenuContent>
    </DropdownMenu>
  )
}

/** Title for a block inside a page: serif, one size everywhere. */
export function SectionHeader({ title, description, actions, className }: { title: string; description?: string; actions?: ReactNode; className?: string }) {
  return (
    <div className={cn('flex flex-wrap items-end justify-between gap-3', className)}>
      <div className="min-w-0 space-y-0.5">
        <h2 className="font-serif text-app-title text-foreground">{title}</h2>
        {description && <p className="text-app-caption text-muted-foreground">{description}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </div>
  )
}

export function SearchInput({
  value,
  onChange,
  placeholder = 'Search',
  label,
  className,
}: {
  value: string
  onChange: (value: string) => void
  placeholder?: string
  label?: string
  className?: string
}) {
  return (
    <div className={cn('relative', className)}>
      <Search size={14} strokeWidth={iconStroke} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground" />
      <Input value={value} onChange={(event) => onChange(event.target.value)} placeholder={placeholder} aria-label={label ?? placeholder} className="pl-8" />
    </div>
  )
}

export function PageLoader({ label = 'Loading' }: { label?: string }) {
  return (
    <div className="space-y-4 py-8" role="status" aria-label={label}>
      <Skeleton className="h-8 w-1/3" />
      <Skeleton className="h-4 w-2/3" />
      <div className="grid gap-3 pt-2 sm:grid-cols-2">
        <Skeleton className="h-20 rounded-xl" />
        <Skeleton className="h-20 rounded-xl" />
        <Skeleton className="h-20 rounded-xl" />
        <Skeleton className="h-20 rounded-xl" />
      </div>
      <span className="sr-only">{label}</span>
    </div>
  )
}

export function EmptyState({
  icon: Icon,
  title,
  description,
  action,
  className,
}: {
  icon?: LucideIcon
  title: string
  description?: string
  action?: ReactNode
  className?: string
}) {
  return (
    <div className={cn('flex flex-col items-center gap-2 rounded-xl border border-dashed border-border bg-surface/50 px-6 py-12 text-center', className)}>
      {Icon && (
        <span className="mb-1 flex size-11 items-center justify-center rounded-xl bg-brand/10 text-brand">
          <Icon size={22} strokeWidth={iconStroke} />
        </span>
      )}
      <p className="text-app-label font-medium text-foreground">{title}</p>
      {description && <p className="max-w-sm text-app-caption text-muted-foreground">{description}</p>}
      {action && <div className="pt-2">{action}</div>}
    </div>
  )
}

/** A clickable list row: title, optional meta line and trailing badge, with hover and selected states. */
export function ListItemCard({
  title,
  meta,
  badge,
  selected,
  onClick,
  className,
}: {
  title: ReactNode
  meta?: ReactNode
  badge?: ReactNode
  selected?: boolean
  onClick?: () => void
  className?: string
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={selected ? 'true' : undefined}
      className={cn(
        'flex w-full items-center gap-3 rounded-xl border px-4 py-3 text-left transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40',
        selected ? 'border-brand/40 bg-accent' : 'border-border bg-surface hover:border-foreground/25 hover:bg-accent/40',
        className,
      )}
    >
      <span className="min-w-0 flex-1">
        <span className="block truncate text-app-label font-medium text-foreground">{title}</span>
        {meta && <span className="mt-0.5 block truncate text-app-caption text-muted-foreground">{meta}</span>}
      </span>
      {badge}
      <ChevronRight size={15} strokeWidth={iconStroke} className="shrink-0 text-muted-foreground" />
    </button>
  )
}

/** Label + control + optional one-line hint. Pass the control's id so the label focuses it. */
export function FormField({
  label,
  htmlFor,
  hint,
  children,
  className,
}: {
  label: string
  htmlFor?: string
  hint?: string
  children: ReactNode
  className?: string
}) {
  return (
    <div className={cn('space-y-1.5', className)}>
      <Label htmlFor={htmlFor}>{label}</Label>
      {children}
      {hint && <p className="text-app-caption text-muted-foreground">{hint}</p>}
    </div>
  )
}

export function CopyField({ value, label = 'Copy' }: { value: string; label?: string }) {
  const [copied, setCopied] = useState(false)
  const copy = () =>
    navigator.clipboard.writeText(value).then(
      () => {
        setCopied(true)
        window.setTimeout(() => setCopied(false), 1500)
      },
      () => undefined,
    )
  return (
    <div className="flex items-center gap-2">
      <Input readOnly value={value} className="font-mono text-app-caption" aria-label={label} onFocus={(event) => event.currentTarget.select()} />
      <Hint text={copied ? 'Copied' : label}>
        <Button type="button" variant="outline" size="icon" aria-label={label} onClick={() => void copy()}>
          {copied ? <Check strokeWidth={iconStroke} /> : <Copy strokeWidth={iconStroke} />}
        </Button>
      </Hint>
    </div>
  )
}

/** Controlled destructive confirmation, for deletes triggered from a menu item (ConfirmAction needs an inline trigger). */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel = 'Delete',
  onConfirm,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description: string
  confirmLabel?: string
  onConfirm: () => void
}) {
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={onConfirm} className="bg-destructive text-destructive-foreground hover:bg-destructive/90">
            {confirmLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

/** Icon-only switch between card and list views. */
export function ViewToggle({ value, onChange }: { value: 'cards' | 'list'; onChange: (value: 'cards' | 'list') => void }) {
  const options = [
    { value: 'cards' as const, label: 'Show as cards', icon: LayoutGrid },
    { value: 'list' as const, label: 'Show as list', icon: List },
  ]
  return (
    <div className="inline-flex rounded-lg border border-border bg-muted/50 p-0.5" role="group" aria-label="View">
      {options.map((option) => (
        <Hint key={option.value} text={option.label}>
          <button
            type="button"
            aria-label={option.label}
            aria-pressed={value === option.value}
            onClick={() => onChange(option.value)}
            className={cn(
              'flex h-7 w-8 items-center justify-center rounded-md transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40',
              value === option.value ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground',
            )}
          >
            <option.icon size={15} strokeWidth={iconStroke} />
          </button>
        </Hint>
      ))}
    </div>
  )
}
