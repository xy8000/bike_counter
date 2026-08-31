import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { cn } from '@/lib/utils'
import { TIMEFRAMES, TIMEFRAME_ORDER } from '../stationDetail/timeframes'
import type { Timeframe } from '../stationDetail/types'
import { useTrendSettings } from './TrendSettingsContext'
import { TrendSettingsIllustration } from './TrendSettingsIllustration'
import type { TimeframeSettingsValue } from './useTimeframeSettings'

/// The timeframe options rendered as one-click choices (the "dropdown" of the
/// settings): the four fixed intervals plus the "Individual" date range.
const TIMEFRAME_OPTIONS: { value: Timeframe; label: string }[] = [
  ...TIMEFRAME_ORDER.map((key) => ({ value: key as Timeframe, label: TIMEFRAMES[key].label })),
  { value: 'individual', label: 'Individual' },
]

/// The Bike-Trends settings dialogue shared by the detail and summary pages:
/// the timeframe (one-click options incl. "Individual" with two date pickers),
/// the compare-previous checkbox (disabled for a custom range) and the
/// exclude-new-stations switch. Every change applies immediately; the Apply
/// button just closes the dialogue.
export function SettingsDialog({
  open,
  onOpenChange,
  settings,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  settings: TimeframeSettingsValue
}) {
  const { excludeNewStations, setExcludeNewStations } = useTrendSettings()
  const {
    timeframe,
    from,
    to,
    compare,
    isIndividual,
    setTimeframe,
    setFrom,
    setTo,
    setCompare,
    setExclude,
  } = settings

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Bike-Trends settings</DialogTitle>
          <DialogDescription>
            Choose the period, the comparison and how the trend metrics are generated.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div>
            <span className="text-sm font-medium">Period</span>
            <div className="mt-2 flex flex-wrap gap-2">
              {TIMEFRAME_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => setTimeframe(option.value)}
                  aria-pressed={timeframe === option.value}
                  className={cn(
                    'rounded-md border px-3 py-1.5 text-sm transition-colors',
                    timeframe === option.value
                      ? 'border-primary bg-primary text-primary-foreground'
                      : 'hover:bg-muted',
                  )}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </div>

          {isIndividual && (
            <div className="grid grid-cols-1 gap-3 rounded-lg border p-3 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label htmlFor="from-date">From</Label>
                <Input
                  id="from-date"
                  type="date"
                  value={from ?? ''}
                  onChange={(event) => setFrom(event.target.value)}
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="to-date">To</Label>
                <Input
                  id="to-date"
                  type="date"
                  value={to ?? ''}
                  onChange={(event) => setTo(event.target.value)}
                />
              </div>
            </div>
          )}

          <div
            className={cn(
              'flex items-center gap-2',
              isIndividual && 'pointer-events-none opacity-50',
            )}
          >
            <Checkbox
              id="compare-previous"
              checked={compare}
              disabled={isIndividual}
              onCheckedChange={(checked) => setCompare(checked === true)}
            />
            <Label
              htmlFor="compare-previous"
              className={cn('cursor-pointer', isIndividual && 'cursor-not-allowed')}
            >
              Compare previous period
            </Label>
          </div>
          {isIndividual && (
            <p className="text-xs text-muted-foreground">
              Comparing to a previous period is not available for an individual date range.
            </p>
          )}
        </div>

        <div className="flex items-start gap-3 rounded-lg border p-3">
          <Switch
            id="exclude-new-stations"
            checked={excludeNewStations}
            onCheckedChange={(checked) => {
              const value = checked === true
              // Keep the app-global flag (header + cards + localStorage) and the
              // shareable URL in sync, so a shared link restores the filter.
              setExcludeNewStations(value)
              setExclude(value)
            }}
            className="mt-0.5"
          />
          <Label
            htmlFor="exclude-new-stations"
            className="min-w-0 cursor-pointer flex-col items-start gap-0.5 leading-snug"
          >
            <span className="whitespace-nowrap">Exclude new stations from trends</span>
            <span className="text-xs font-normal text-muted-foreground">
              Only include stations with data covering the whole current (and, when comparing, the
              whole previous) period, so newly-built stations cannot skew the trends.
            </span>
          </Label>
        </div>

        <TrendSettingsIllustration excludeNewStations={excludeNewStations} />

        <DialogFooter>
          <Button type="button" onClick={() => onOpenChange(false)}>
            Apply
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
