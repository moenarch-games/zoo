// Browser side of the #87 session recorder (Atlas 4.1.2-4.1.5). zoo-core records the
// authoritative session; this module only retains its latest bundle, adds bounded
// browser-side failure records and builds the export document. Nothing is uploaded.

export type FailureSource = "wasm" | "renderer" | "asset" | "storage" | "application"
export type Recovery = "frozen" | "continue_paused" | "manual_export"

export type BrowserFailure = {
  source: FailureSource
  message: string
  recovery: Recovery
  tick: number | null
}

const MAX_BROWSER_FAILURES = 50
const MAX_FAILURE_MESSAGE = 2_000
export const MAX_EXPORT_BYTES = 1_048_576

type SessionReader = () => string

export class SessionDiagnostics {
  private failures: BrowserFailure[] = []
  private retained: string | null = null
  private reader: SessionReader | null = null

  /** Connects the live zoo-core bundle; `null` after the simulation can no longer be called. */
  connect(reader: SessionReader | null) {
    this.reader = reader
  }

  /** Refreshes the retained copy while the simulation is callable (after commands, not per tick). */
  retain() {
    if (!this.reader) {
      return
    }
    try {
      this.retained = this.reader()
    } catch {
      // A trapped simulation keeps the last retained bundle.
    }
  }

  private sessionText() {
    this.retain()
    return this.retained
  }

  private tick(): number | null {
    if (!this.retained) {
      return null
    }
    try {
      const tick = (JSON.parse(this.retained) as { clock?: { tick?: unknown } }).clock?.tick
      return typeof tick === "number" ? tick : null
    } catch {
      return null
    }
  }

  /** Records a browser failure once per source and message; returns whether it was new. */
  record(source: FailureSource, message: string, recovery: Recovery) {
    const text = message.slice(0, MAX_FAILURE_MESSAGE)
    if (this.failures.some((failure) => failure.source === source && failure.message === text)) {
      return false
    }
    this.failures.push({ source, message: text, recovery, tick: this.tick() })
    if (this.failures.length > MAX_BROWSER_FAILURES) {
      this.failures.shift()
    }
    return true
  }

  /** The exact text that Download and Copy export. */
  exportText() {
    const session = this.sessionText()
    const failures = [...this.failures]
    for (;;) {
      const text = `{"session":${session ?? "null"},"browser_failures":${JSON.stringify(failures)},"screenshot":null}`
      if (new TextEncoder().encode(text).length <= MAX_EXPORT_BYTES || failures.length === 0) {
        return text
      }
      failures.shift()
    }
  }
}

export const diagnostics = new SessionDiagnostics()

declare global {
  // oxlint-disable-next-line typescript/consistent-type-definitions -- global augmentation requires interface merging.
  interface Window {
    __zooDiagnostics?: { exportText: () => string }
    __zooFaults?: {
      simulation: (kind: string) => string
      /** Unmounts and remounts the park view, exercising the renderer's full load/dispose cycle. */
      remountParkView: () => void
    }
  }
}

export function installDiagnosticsHook() {
  window.__zooDiagnostics = { exportText: () => diagnostics.exportText() }
}

/** Test-only fault injection, present only with `?zoo-fault-injection=1`. */
export function faultInjectionEnabled() {
  try {
    return new URLSearchParams(window.location.search).get("zoo-fault-injection") === "1"
  } catch {
    return false
  }
}
