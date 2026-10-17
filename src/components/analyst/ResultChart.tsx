import { useEffect, useMemo, useRef, useState } from 'react'
import { BarChart, FunnelChart, HeatmapChart, LineChart, PieChart } from 'echarts/charts'
import { GridComponent, LegendComponent, TooltipComponent, VisualMapComponent } from 'echarts/components'
import * as echarts from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import type { ChartSpec, QueryResult, SampleRow } from '../../lib/api'
import { Button } from '../ui/button'
import { Card } from '../ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../ui/table'
import { Hint } from '../ui/tooltip'
import { PaginationBar } from '../ui/pagination'
import { usePagination } from '../../lib/usePagination'
import { formatCell, SampleTable } from '../data/SampleTable'

echarts.use([BarChart, FunnelChart, HeatmapChart, LineChart, PieChart, GridComponent, LegendComponent, TooltipComponent, VisualMapComponent, CanvasRenderer])

// Categorical palette, kept apart from the status colours (success/warning/destructive).
const SERIES_TOKENS = ['chart-1', 'chart-2', 'chart-3', 'chart-4', 'chart-5', 'chart-6', 'chart-7', 'chart-8']
// A second cue besides colour, so series stay distinguishable without colour vision.
const SYMBOLS = ['circle', 'rect', 'triangle', 'diamond', 'roundRect', 'pin', 'arrow', 'circle']
const FONT = '"Inter Variable", -apple-system, system-ui, sans-serif'
const VALUE_LABELS_UP_TO = 8
const compact = new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 })
const DOTS_UP_TO = 60

/** ECharts needs pixels, so the caption size from the theme scale (rem, scaled by the font-size setting) is resolved here. */
function captionPx() {
  const root = getComputedStyle(document.documentElement)
  const rem = parseFloat(root.getPropertyValue('--app-text-caption')) || 0.75
  return Math.round(rem * (parseFloat(root.fontSize) || 16))
}

/** ECharts parses only comma-separated colours, so theme tokens (`15 56% 52%`) are converted. */
function token(name: string, alpha = 1) {
  const [h, s, l] = getComputedStyle(document.documentElement).getPropertyValue(`--${name}`).trim().split(/\s+/)
  return `hsla(${h}, ${s}, ${l}, ${alpha})`
}

function toNumber(value: unknown): number | null {
  if (typeof value === 'number') return value
  if (typeof value === 'string' && value.trim() !== '') {
    const parsed = Number(value)
    return Number.isFinite(parsed) ? parsed : null
  }
  return null
}

export function formatNumber(value: number) {
  return value.toLocaleString(undefined, { maximumFractionDigits: 2 })
}

/** Re-renders when the light/dark class on the root element changes. */
function useThemeVersion() {
  const [version, setVersion] = useState(0)
  useEffect(() => {
    const observer = new MutationObserver(() => setVersion((current) => current + 1))
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'data-font-size'] })
    return () => observer.disconnect()
  }, [])
  return version
}

function buildOption(spec: ChartSpec, result: QueryResult): echarts.EChartsCoreOption {
  const index = (name: string | null) => result.columns.findIndex((column) => column.name === name)
  const xIndex = index(spec.x)
  const labels = result.rows.map((row) => formatCell(row[xIndex]))
  const text = token('muted-foreground')
  const size = captionPx()
  const tooltip = {
    backgroundColor: token('popover'),
    borderColor: token('border'),
    borderWidth: 1,
    padding: [8, 10],
    extraCssText: 'border-radius: 10px; box-shadow: 0 8px 24px -8px rgba(0,0,0,0.25);',
    textStyle: { color: token('foreground'), fontSize: size, fontFamily: FONT },
    valueFormatter: (value: unknown) => (typeof value === 'number' ? formatNumber(value) : String(value)),
  }
  const colors = SERIES_TOKENS.map((name) => token(name))
  const base = { textStyle: { fontFamily: FONT }, animationDuration: 400, animationEasing: 'cubicOut' as const }

  if (spec.kind === 'funnel' || spec.kind === 'pie') {
    const yIndex = index(spec.y[0])
    return {
      ...base,
      color: colors,
      tooltip: { ...tooltip, trigger: 'item' },
      series: [
        {
          type: spec.kind,
          radius: spec.kind === 'pie' ? ['42%', '70%'] : undefined,
          itemStyle: { borderColor: token('surface'), borderWidth: 2, borderRadius: spec.kind === 'pie' ? 6 : 0 },
          label: { color: token('foreground'), fontSize: size, formatter: spec.kind === 'pie' ? '{b}: {d}%' : '{b}' },
          data: result.rows.map((row, i) => ({ name: labels[i], value: toNumber(row[yIndex]) })),
        },
      ],
    }
  }

  if (spec.kind === 'heatmap') {
    const yName = spec.y[0]
    const valueName = spec.y[1]
    const yIndex = index(yName)
    const valueIndex = index(valueName)
    const yLabels = [...new Set(result.rows.map((row) => formatCell(row[yIndex])))]
    const data = result.rows.map((row) => [
      labels[result.rows.indexOf(row)] ?? formatCell(row[xIndex]),
      formatCell(row[yIndex]),
      toNumber(row[valueIndex]),
    ])
    return {
      ...base,
      tooltip,
      visualMap: { min: 0, max: Math.max(1, ...data.map((point) => Number(point[2] ?? 0))), inRange: { color: [token('muted'), token('brand')] }, textStyle: { color: text } },
      grid: { left: 8, right: 16, top: 12, bottom: 8, containLabel: true },
      xAxis: { type: 'category', data: [...new Set(labels)], axisLabel: { color: text }, axisLine: { lineStyle: { color: token('border') } } },
      yAxis: { type: 'category', data: yLabels, axisLabel: { color: text } },
      series: [{ type: 'heatmap', data }],
    }
  }

  const type = spec.kind === 'line' || spec.kind === 'area' ? 'line' : 'bar'
  const showValues = type === 'bar' && spec.y.length === 1 && result.rows.length <= VALUE_LABELS_UP_TO
  const grid = token('border', 0.8)
  return {
    ...base,
    color: colors,
    tooltip: { ...tooltip, trigger: 'axis', axisPointer: { type: type === 'bar' ? 'shadow' : 'line', shadowStyle: { color: token('muted', 0.5) }, lineStyle: { color: token('border') } } },
    legend:
      spec.y.length > 1
        ? { top: 0, icon: 'roundRect', itemWidth: 10, itemHeight: 10, itemGap: 16, textStyle: { color: text, fontSize: size, fontFamily: FONT } }
        : undefined,
    grid: { left: 8, right: 16, top: spec.y.length > 1 ? 32 : 12, bottom: 8, containLabel: true },
    xAxis: {
      type: 'category',
      data: labels,
      axisLine: { lineStyle: { color: token('border') } },
      axisTick: { show: false },
      axisLabel: { color: text, hideOverlap: true, fontSize: size },
    },
    yAxis: {
      type: 'value',
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { color: text, fontSize: size, formatter: (value: number) => compact.format(value) },
      splitLine: { lineStyle: { color: grid, type: 'dashed' } },
    },
    series: spec.y.map((name, seriesIndex) => {
      const yIndex = index(name)
      return {
        type,
        name,
        stack: spec.kind === 'stacked' ? 'total' : undefined,
        areaStyle: spec.kind === 'area' ? { opacity: 0.15 } : undefined,
        data: result.rows.map((row) => toNumber(row[yIndex])),
        lineStyle: { width: 2.5, ...(spec.dashed?.includes(name) ? { type: 'dashed' } : {}) },
        symbol: SYMBOLS[seriesIndex % SYMBOLS.length],
        symbolSize: 7,
        showSymbol: result.rows.length <= DOTS_UP_TO,
        itemStyle: type === 'bar' && spec.kind !== 'stacked' ? { borderRadius: [4, 4, 0, 0] } : undefined,
        label: showValues ? { show: true, position: 'top', color: text, fontSize: size, formatter: (params: { value: number }) => compact.format(params.value) } : undefined,
        emphasis: { focus: 'series' },
        barMaxWidth: 36,
      }
    }),
  }
}

function EChart({ spec, result, onSelect }: { spec: ChartSpec; result: QueryResult; onSelect?: (name: string) => void }) {
  const container = useRef<HTMLDivElement>(null)
  const chart = useRef<echarts.ECharts | null>(null)
  const themeVersion = useThemeVersion()

  useEffect(() => {
    const element = container.current
    if (!element) return
    const instance = echarts.init(element)
    chart.current = instance
    const resize = new ResizeObserver(() => instance.resize())
    resize.observe(element)
    return () => {
      resize.disconnect()
      instance.dispose()
      chart.current = null
    }
  }, [])

  useEffect(() => {
    const instance = chart.current
    if (!instance) return
    instance.setOption(buildOption(spec, result), true)
    instance.off('click')
    if (onSelect) {
      instance.on('click', (params: { name?: string }) => {
        if (params.name) onSelect(params.name)
      })
    }
  }, [spec, result, themeVersion, onSelect])

  return <div ref={container} className="h-full min-h-[12rem] w-full" />
}

function Kpis({ spec, result }: { spec: ChartSpec; result: QueryResult }) {
  const row = result.rows[0] ?? []
  return (
    <div className="flex flex-wrap gap-3">
      {spec.y.map((name) => {
        const value = row[result.columns.findIndex((column) => column.name === name)]
        const number = toNumber(value)
        return (
          <Card key={name} className="min-w-[10rem] px-4 py-3">
            <p className="text-app-caption text-muted-foreground">{name.replace(/_/g, ' ')}</p>
            <p className="mt-1 font-serif text-app-page tabular-nums text-foreground">{number === null ? formatCell(value) : formatNumber(number)}</p>
          </Card>
        )
      })}
    </div>
  )
}

export function ResultTable({ result, onSelectCell }: { result: QueryResult; onSelectCell?: (column: string, value: string) => void }) {
  const columns = result.columns.map((column) => column.name)
  const rows = useMemo(
    () => result.rows.map((row) => Object.fromEntries(result.columns.map((column, i) => [column.name, row[i]])) as SampleRow),
    [result],
  )
  const pg = usePagination(rows, { initialPageSize: 10, resetKey: rows })
  return (
    <div className="space-y-1.5">
      {onSelectCell ? (
        <div className="space-y-1">
        <div className="max-h-80 overflow-auto rounded-xl">
          <Table className="font-mono text-app-caption">
            <TableHeader>
              <TableRow>
                {columns.map((column) => (
                  <TableHead key={column} className="whitespace-nowrap">
                    {column}
                  </TableHead>
                ))}
              </TableRow>
            </TableHeader>
            <TableBody>
              {pg.pageItems.map((row, index) => (
                <TableRow key={(pg.page - 1) * pg.pageSize + index}>
                  {columns.map((column) => {
                    const value = row[column]
                    const empty = value === null || value === undefined
                    const cell = (
                      <TableCell
                        key={column}
                        onClick={empty ? undefined : () => onSelectCell(column, String(value))}
                        className={empty ? 'text-muted-foreground/60' : 'max-w-[18rem] cursor-pointer truncate text-foreground hover:bg-accent'}
                      >
                        {formatCell(value)}
                      </TableCell>
                    )
                    return empty ? (
                      cell
                    ) : (
                      <Hint key={column} text={`Filter the dashboard to ${column} = ${formatCell(value)}`}>
                        {cell}
                      </Hint>
                    )
                  })}
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
          <PaginationBar page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} className="pt-1" pageSizeOptions={[10, 25, 50, 100]} />
        </div>
      ) : (
        <SampleTable columns={columns} rows={rows} />
      )}
      {result.truncated && <p className="text-app-caption text-muted-foreground">Showing the first {result.rows.length.toLocaleString()} rows.</p>}
    </div>
  )
}

export function ResultChart({
  spec,
  result,
  onSelect,
  onSelectCell,
  tall,
}: {
  spec: ChartSpec
  result: QueryResult
  onSelect?: (name: string) => void
  onSelectCell?: (column: string, value: string) => void
  /** Gives the chart room to read, for a chart shown on its own. */
  tall?: boolean
}) {
  const [asTable, setAsTable] = useState(false)
  if (spec.kind === 'table') return <ResultTable result={result} onSelectCell={onSelectCell} />
  if (spec.kind === 'text') return null
  if (spec.kind === 'kpi') return <Kpis spec={spec} result={result} />
  return (
    <div className="space-y-2">
      <div className="flex justify-end gap-1">
        {(['Chart', 'Table'] as const).map((label) => {
          const active = (label === 'Table') === asTable
          return (
            <Button
              key={label}
              type="button"
              variant={active ? 'secondary' : 'ghost'}
              size="sm"
              onClick={() => setAsTable(label === 'Table')}
              aria-pressed={active}
            >
              {label}
            </Button>
          )
        })}
      </div>
      {asTable ? (
        <ResultTable result={result} onSelectCell={onSelectCell} />
      ) : (
        <div className={`rounded-xl border border-border bg-surface p-3${tall ? ' h-[28rem]' : ''}`}>
          <EChart spec={spec} result={result} onSelect={onSelect} />
        </div>
      )}
    </div>
  )
}
