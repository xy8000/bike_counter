import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { useTrendSettings } from './TrendSettingsContext'

/// The Bike-Trends settings dialogue: toggles whether new stations (those
/// without measurements covering the whole compared period) are excluded from
/// the trend metrics and the current-vs-previous graph comparisons.
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
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Bike-Trends settings</DialogTitle>
          <DialogDescription>
            Choose how the trend metrics and comparison graphs are generated.
          </DialogDescription>
        </DialogHeader>
        <div className="flex items-start gap-2">
          <Checkbox
            id="exclude-new-stations"
            checked={excludeNewStations}
            onCheckedChange={(checked) => setExcludeNewStations(checked === true)}
          />
          <Label htmlFor="exclude-new-stations" className="cursor-pointer leading-snug">
            Exclude new stations from trends
            <span className="block text-xs font-normal text-muted-foreground">
              Only include stations with data covering the whole current (and, when comparing, the
              whole previous) period, so newly-built stations cannot skew the trends.
            </span>
          </Label>
        </div>
      </DialogContent>
    </Dialog>
  )
}
