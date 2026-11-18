import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '../../lib/utils'

const alertVariants = cva('relative flex w-full items-start gap-3 rounded-lg border px-4 py-3 text-app-label [&>svg]:mt-0.5 [&>svg]:size-4 [&>svg]:shrink-0', {
  variants: {
    variant: {
      default: 'border-border bg-muted/40 text-foreground',
      warning: 'border-warning/30 bg-warning/10 text-foreground [&>svg]:text-warning',
      destructive: 'border-destructive/30 bg-destructive/10 text-foreground [&>svg]:text-destructive',
    },
  },
  defaultVariants: { variant: 'default' },
})

const Alert = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement> & VariantProps<typeof alertVariants>>(
  ({ className, variant, ...props }, ref) => <div ref={ref} role="alert" className={cn(alertVariants({ variant }), className)} {...props} />,
)
Alert.displayName = 'Alert'


const AlertDescription = React.forwardRef<HTMLParagraphElement, React.HTMLAttributes<HTMLParagraphElement>>(({ className, ...props }, ref) => (
  <div ref={ref} className={cn('text-app-caption text-muted-foreground', className)} {...props} />
))
AlertDescription.displayName = 'AlertDescription'

export { Alert, AlertDescription }
