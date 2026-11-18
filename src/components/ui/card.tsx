import * as React from 'react'
import { cn } from '../../lib/utils'

const CARD_VARIANTS = {
  default: '',
  raised: 'shadow-sm',
  interactive: 'transition-colors duration-fast hover:border-foreground/25 hover:bg-accent/40',
} as const

const Card = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement> & { variant?: keyof typeof CARD_VARIANTS }>(
  ({ className, variant = 'default', ...props }, ref) => (
    <div ref={ref} className={cn('rounded-lg border border-border bg-surface text-foreground', CARD_VARIANTS[variant], className)} {...props} />
  ),
)
Card.displayName = 'Card'






export { Card }
