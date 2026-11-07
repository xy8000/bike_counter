import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { FRONTEND_VERSION } from '../../lib/version'

/// The backend version label: the fetched version, the error fallback, or the
/// in-flight placeholder while `GET /api/bff/version` is still pending.
function backendVersionLabel(backendVersion: string | null, error: boolean): string {
  if (backendVersion) {
    return `v${backendVersion}`
  }
  if (error) {
    return 'Unavailable'
  }
  return 'Loading…'
}

/// Popup dialogue for the installed application versions. The frontend version
/// is baked into the bundle at build time; the backend version is fetched from
/// `GET /api/bff/version` and shows a loading/error fallback until it arrives.
export function VersionDialog({
  backendVersion,
  error,
  onClose,
}: Readonly<{
  backendVersion: string | null
  error: boolean
  onClose: () => void
}>) {
  const backendLabel = backendVersionLabel(backendVersion, error)

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Versions</DialogTitle>
          <DialogDescription>Currently installed application versions.</DialogDescription>
        </DialogHeader>
        <dl className="flex flex-col gap-3 text-sm">
          <div className="flex items-baseline justify-between gap-4">
            <dt className="text-xs font-medium text-muted-foreground">Frontend</dt>
            <dd className="font-medium">v{FRONTEND_VERSION}</dd>
          </div>
          <div className="flex items-baseline justify-between gap-4">
            <dt className="text-xs font-medium text-muted-foreground">Backend</dt>
            <dd className="font-medium">{backendLabel}</dd>
          </div>
        </dl>
      </DialogContent>
    </Dialog>
  )
}
