import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { useTrendSettings } from './TrendSettingsContext'
import { TrendSettingsIllustration } from './TrendSettingsIllustration'

/// The Bike-Trends settings dialogue: toggles whether new stations (those
/// without measurements covering the whole compared period) are excluded from
/// the trend metrics and the current-vs-previous graph comparisons. The
/// side-by-side illustration re-computes with the toggle so the effect of the
/// setting is visible at a glance.
export function SettingsDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { excludeNewStations, setExcludeNewStations } = useTrendSettings()

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Bike-Trends settings</DialogTitle>
          <DialogDescription>
            Choose how the trend metrics and comparison graphs are generated.
          </DialogDescription>
        </DialogHeader>

        <div className="flex items-start gap-3 rounded-lg border p-3">
          <Switch
            id="exclude-new-stations"
            checked={excludeNewStations}
            onCheckedChange={(checked) => setExcludeNewStations(checked === true)}
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
      </DialogContent>
    </Dialog>
  )
}
