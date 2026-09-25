import { useState } from 'react'
import { SlidersHorizontal } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { SettingsDialog } from './SettingsDialog'
import { TimeframeSettingsLabel } from './TimeframeSettingsLabel'
import type { TimeframeSettingsValue } from './useTimeframeSettings'

/// The control cluster to the right of a "Detailed statistics" page heading: the
/// short summary of the current timeframe settings, the button that opens the
/// Bike-Trends settings dialogue and the dialogue itself. Both the station-detail
/// and the station-summary page render this exact cluster, so it (including its
/// open/closed state) lives here instead of being duplicated in both pages.
export function TimeframeSettingsControls({
  settings,
}: Readonly<{ settings: TimeframeSettingsValue }>) {
  const [settingsOpen, setSettingsOpen] = useState(false)

  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
      <TimeframeSettingsLabel settings={settings} />
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => setSettingsOpen(true)}
        title="Calculation settings"
        aria-label="Calculation settings"
      >
        <SlidersHorizontal />
        Settings
      </Button>
      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} settings={settings} />
    </div>
  )
}
