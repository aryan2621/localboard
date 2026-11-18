import * as ProgressPrimitive from '@radix-ui/react-progress'
import { cn } from '../../lib/utils'

const TONES = { brand: 'bg-brand', success: 'bg-success', warning: 'bg-warning', destructive: 'bg-destructive' }

export function Progress({ value, tone = 'brand', className }: { value: number; tone?: keyof typeof TONES; className?: string }) {
  const pct = Math.max(0, Math.min(100, value))
  return (
    <ProgressPrimitive.Root value={pct} className={cn('h-1.5 min-w-0 overflow-hidden rounded-full bg-muted', className)}>
      <ProgressPrimitive.Indicator
        className={cn('h-full w-full origin-left rounded-full transition-transform duration-base', TONES[tone])}
        style={{ transform: `scaleX(${pct / 100})` }}
      />
    </ProgressPrimitive.Root>
  )
}
