import defaultTheme from 'tailwindcss/defaultTheme'

/** @type {import('tailwindcss').Config} */
export default {
  darkMode: ['class'],
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        border: 'hsl(var(--border))',
        input: 'hsl(var(--input))',
        ring: 'hsl(var(--ring))',
        background: 'hsl(var(--background))',
        foreground: 'hsl(var(--foreground))',
        primary: {
          DEFAULT: 'hsl(var(--primary))',
          foreground: 'hsl(var(--primary-foreground))',
        },
        secondary: {
          DEFAULT: 'hsl(var(--secondary))',
          foreground: 'hsl(var(--secondary-foreground))',
        },
        destructive: {
          DEFAULT: 'hsl(var(--destructive))',
          foreground: 'hsl(var(--destructive-foreground))',
        },
        muted: {
          DEFAULT: 'hsl(var(--muted))',
          foreground: 'hsl(var(--muted-foreground))',
        },
        accent: {
          DEFAULT: 'hsl(var(--accent))',
          foreground: 'hsl(var(--accent-foreground))',
        },
        popover: {
          DEFAULT: 'hsl(var(--popover))',
          foreground: 'hsl(var(--popover-foreground))',
        },
        card: {
          DEFAULT: 'hsl(var(--card))',
          foreground: 'hsl(var(--card-foreground))',
        },
        sidebar: 'hsl(var(--sidebar))',
        surface: {
          DEFAULT: 'hsl(var(--surface))',
          foreground: 'hsl(var(--surface-foreground))',
        },
        chart: {
          1: 'hsl(var(--chart-1))',
          2: 'hsl(var(--chart-2))',
          3: 'hsl(var(--chart-3))',
          4: 'hsl(var(--chart-4))',
          5: 'hsl(var(--chart-5))',
          6: 'hsl(var(--chart-6))',
          7: 'hsl(var(--chart-7))',
          8: 'hsl(var(--chart-8))',
        },
        brand: {
          DEFAULT: 'hsl(var(--brand))',
          foreground: 'hsl(var(--brand-foreground))',
        },
        success: 'hsl(var(--success))',
        warning: 'hsl(var(--warning))',
      },
      borderRadius: {
        lg: 'var(--radius)',
        md: 'calc(var(--radius) - 2px)',
        sm: 'calc(var(--radius) - 4px)',
      },
      fontFamily: {
        serif: ['"Source Serif 4 Variable"', 'Georgia', ...defaultTheme.fontFamily.serif],
        sans: ['"Inter Variable"', '-apple-system', 'BlinkMacSystemFont', 'system-ui', ...defaultTheme.fontFamily.sans],
        mono: ['"JetBrains Mono Variable"', 'ui-monospace', 'SFMono-Regular', 'Menlo', ...defaultTheme.fontFamily.mono],
      },
      fontSize: {
        'app-caption': ['var(--app-text-caption)', { lineHeight: '1.33' }],
        'app-label': ['var(--app-text-label)', { lineHeight: '1.43' }],
        'app-body': ['var(--app-text-body)', { lineHeight: '1.5' }],
        'app-title': ['var(--app-text-title)', { lineHeight: '1.3' }],
        'app-heading': ['var(--app-text-heading)', { lineHeight: '1.2' }],
        'app-page': ['var(--app-text-page)', { lineHeight: '1.2' }],
        'app-display': ['var(--app-text-display)', { lineHeight: '1.11' }],
      },
      transitionDuration: {
        fast: '150ms',
        base: '220ms',
      },
      transitionTimingFunction: {
        'out-soft': 'cubic-bezier(0.16, 1, 0.3, 1)',
      },
      boxShadow: {
        xs: '0 1px 2px 0 hsl(var(--shadow-color) / 0.05)',
        sm: '0 1px 3px 0 hsl(var(--shadow-color) / 0.08), 0 1px 2px -1px hsl(var(--shadow-color) / 0.06)',
        md: '0 4px 12px -2px hsl(var(--shadow-color) / 0.10), 0 2px 4px -2px hsl(var(--shadow-color) / 0.06)',
        lg: '0 12px 32px -8px hsl(var(--shadow-color) / 0.18), 0 4px 8px -4px hsl(var(--shadow-color) / 0.08)',
      },
      keyframes: {
        'overlay-in': { from: { opacity: '0' }, to: { opacity: '1' } },
        'overlay-out': { from: { opacity: '1' }, to: { opacity: '0' } },
        'content-in': { from: { opacity: '0', transform: 'translate(-50%, -48%) scale(0.97)' }, to: { opacity: '1', transform: 'translate(-50%, -50%) scale(1)' } },
        'content-out': { from: { opacity: '1', transform: 'translate(-50%, -50%) scale(1)' }, to: { opacity: '0', transform: 'translate(-50%, -48%) scale(0.97)' } },
        'pop-in': { from: { opacity: '0', transform: 'scale(0.96)' }, to: { opacity: '1', transform: 'scale(1)' } },
        'pop-out': { from: { opacity: '1', transform: 'scale(1)' }, to: { opacity: '0', transform: 'scale(0.96)' } },
        'rise-in': { from: { opacity: '0', transform: 'translateY(6px)' }, to: { opacity: '1', transform: 'translateY(0)' } },
        'sheet-in': { from: { transform: 'translateX(100%)' }, to: { transform: 'translateX(0)' } },
        'sheet-out': { from: { transform: 'translateX(0)' }, to: { transform: 'translateX(100%)' } },
        'step-in': { from: { opacity: '0', transform: 'translateY(5px)' }, to: { opacity: '1', transform: 'translateY(0)' } },
        shimmer: { from: { backgroundPosition: '100% 0' }, to: { backgroundPosition: '-100% 0' } },
      },
      animation: {
        'overlay-in': 'overlay-in 150ms ease-out',
        'overlay-out': 'overlay-out 120ms ease-in',
        'content-in': 'content-in 200ms cubic-bezier(0.16, 1, 0.3, 1)',
        'content-out': 'content-out 140ms ease-in',
        'pop-in': 'pop-in 140ms cubic-bezier(0.16, 1, 0.3, 1)',
        'pop-out': 'pop-out 100ms ease-in',
        'rise-in': 'rise-in 220ms cubic-bezier(0.16, 1, 0.3, 1)',
        'sheet-in': 'sheet-in 260ms cubic-bezier(0.16, 1, 0.3, 1)',
        'sheet-out': 'sheet-out 180ms ease-in',
        'step-in': 'step-in 280ms cubic-bezier(0.16, 1, 0.3, 1)',
        shimmer: 'shimmer 2.2s linear infinite',
      },
    },
  },
  plugins: [],
}
