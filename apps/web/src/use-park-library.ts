import { useCallback, useEffect, useRef, useState } from "react"

import {
  type AutosaveCadence,
  background,
  describeSave,
  loadFailureMessage,
  MAX_SAVE_BYTES,
  nameFromFile,
  ParkLibrary,
  readCadence,
  type SaveKind,
  saveFileName,
  type SlotEntry,
  type SlotMeta,
  StorageError,
  summarizeDocument,
  writeCadence,
} from "./park-storage"

export type ActiveSlot = {
  id: string
  name: string
  revision: number
}

export type LoadCheck = {
  ok: boolean
  message: string
  code?: string
}

type Options = {
  /** The active park as a zoo-core save, or undefined while the simulation is stopped. */
  captureSave: () => string | undefined
  /** Validates a document through zoo-core without touching the active park. */
  validateSave: (document: string) => LoadCheck
  notify: (text: string, kind?: "info" | "error") => void
}

/**
 * Browser adapter for the park library (#89): slot bookkeeping, the rotating autosave cadence,
 * lifecycle autosaves, competing-tab conflicts and file import/export. zoo-core produces and
 * validates every document; this hook only decides when and where they are stored.
 */
export function useParkLibrary({ captureSave, validateSave, notify }: Options) {
  const libraryRef = useRef<ParkLibrary | null>(null)
  const [ready, setReady] = useState(false)
  const [storageNotice, setStorageNotice] = useState<string | null>(null)
  const [entries, setEntries] = useState<SlotEntry[]>([])
  const activeRef = useRef<ActiveSlot | null>(null)
  const [active, setActiveState] = useState<ActiveSlot | null>(null)
  const [status, setStatus] = useState("Not saved yet")
  const [conflict, setConflict] = useState<StorageError | null>(null)
  const conflictRef = useRef<StorageError | null>(null)
  const [exportFallback, setExportFallback] = useState<{ name: string; text: string } | null>(null)
  const [cadence, setCadenceState] = useState<AutosaveCadence>(readCadence)
  const changeRef = useRef(0)
  const savedChangeRef = useRef(0)
  const lastAutosaveMinuteRef = useRef(0)
  const queueRef = useRef<Promise<unknown>>(Promise.resolve())

  const setActive = useCallback((slot: ActiveSlot | null) => {
    activeRef.current = slot
    setActiveState(slot)
  }, [])

  const setConflictState = useCallback((error: StorageError | null) => {
    conflictRef.current = error
    setConflict(error)
  }, [])

  const refresh = useCallback(async () => {
    const library = libraryRef.current
    if (!library) {
      return []
    }
    try {
      const list = await library.list()
      setEntries(list)
      return list
    } catch (error) {
      setStorageNotice(error instanceof Error ? error.message : String(error))
      return []
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    ParkLibrary.open()
      .then(async (library) => {
        if (cancelled) {
          return
        }
        libraryRef.current = library
        await refresh()
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setStorageNotice(error instanceof Error ? error.message : String(error))
        }
      })
      .finally(() => {
        if (!cancelled) {
          setReady(true)
        }
      })
    return () => {
      cancelled = true
    }
  }, [refresh])

  /** Writes run one at a time so a lifecycle autosave never races a manual save. */
  const enqueue = useCallback(<T>(work: () => Promise<T>): Promise<T> => {
    const next = queueRef.current.then(work, work)
    queueRef.current = next.catch(() => undefined)
    return next
  }, [])

  const markChanged = useCallback(() => {
    changeRef.current += 1
  }, [])

  const dirty = useCallback(() => changeRef.current !== savedChangeRef.current, [])

  /** Marks the park just created, loaded or saved as the current saved baseline. */
  const adopt = useCallback(
    (slot: SlotMeta | null, absoluteMinute: number) => {
      setActive(slot && { id: slot.id, name: slot.name, revision: slot.revision })
      setConflictState(null)
      changeRef.current = 0
      savedChangeRef.current = 0
      lastAutosaveMinuteRef.current = absoluteMinute
      if (slot?.latest) {
        setStatus(`Saved ${describeSave(slot.latest)}`)
      } else {
        setStatus(
          libraryRef.current
            ? "Not saved yet"
            : "Browser storage unavailable — export to keep this park",
        )
      }
    },
    [setActive, setConflictState],
  )

  const defaultName = useCallback(() => {
    const used = new Set(entries.map((entry) => entry.slot.name))
    let index = entries.length + 1
    while (used.has(`Park ${index}`)) {
      index += 1
    }
    return `Park ${index}`
  }, [entries])

  const failed = useCallback(
    (error: unknown, kind: SaveKind | "create") => {
      const failure =
        error instanceof StorageError
          ? error
          : new StorageError(
              "failed",
              `Saving failed (${String(error)}). The previous save is unchanged.`,
            )
      if (failure.kind === "conflict") {
        setConflictState(failure)
        notify(
          `${failure.message} Nothing was overwritten; choose how to continue in the park menu.`,
          "error",
        )
        setStatus("Save conflict — autosave paused")
      } else {
        notify(failure.message, "error")
        setStatus(
          kind === "auto" ? "Autosave failed — last save kept" : "Save failed — last save kept",
        )
      }
    },
    [notify, setConflictState],
  )

  /** A new slot holding the active park; it becomes the active slot. */
  const createSlot = useCallback(
    (name: string) =>
      enqueue(async () => {
        const library = libraryRef.current
        const document = captureSave()
        if (!document) {
          return null
        }
        const summary = summarizeDocument(document)
        const minute = (summary.day - 1) * 1440 + summary.minute
        if (!library) {
          adopt(null, minute)
          return null
        }
        const change = changeRef.current
        try {
          const slot = await library.create(name, document)
          adopt(slot, minute)
          changeRef.current = change
          savedChangeRef.current = change
          background(refresh())
          return slot
        } catch (error) {
          adopt(null, minute)
          failed(error, "create")
          return null
        }
      }),
    [adopt, captureSave, enqueue, failed, refresh],
  )

  const save = useCallback(
    (kind: SaveKind, overwrite = false): Promise<boolean> =>
      enqueue(async () => {
        const library = libraryRef.current
        const slot = activeRef.current
        if (!library) {
          if (kind === "manual") {
            notify(
              storageNotice ?? "Browser storage is unavailable. Export the park to a file instead.",
              "error",
            )
          }
          return false
        }
        if (!slot) {
          return false
        }
        if (conflictRef.current && !overwrite) {
          if (kind === "manual") {
            notify(
              "Resolve the save conflict in the park menu first; nothing was overwritten.",
              "error",
            )
          }
          return false
        }
        const document = captureSave()
        if (!document) {
          return false
        }
        const change = changeRef.current
        try {
          const next = await library.write(
            slot.id,
            kind,
            document,
            overwrite ? null : slot.revision,
          )
          setActive({ id: next.id, name: next.name, revision: next.revision })
          setConflictState(null)
          savedChangeRef.current = change
          const where = next.latest ? describeSave(next.latest) : ""
          setStatus(`${kind === "auto" ? "Autosaved" : "Saved"} ${where}`)
          if (kind === "manual") {
            notify(`Saved “${next.name}” at ${where}.`)
          }
          background(refresh())
          return true
        } catch (error) {
          failed(error, kind)
          return false
        }
      }),
    [captureSave, enqueue, failed, notify, refresh, setActive, setConflictState, storageNotice],
  )

  /** Called after every simulation tick with the park's absolute in-game minute. */
  const autosaveIfDue = useCallback(
    (absoluteMinute: number) => {
      if (cadence === 0 || !activeRef.current || conflictRef.current) {
        return
      }
      if (absoluteMinute - lastAutosaveMinuteRef.current >= cadence) {
        lastAutosaveMinuteRef.current = absoluteMinute
        background(save("auto"))
      }
    },
    [cadence, save],
  )

  // Lifecycle opportunities: hiding the tab or leaving the page. Unload alone is unreliable,
  // so the cadence above remains the main guarantee.
  useEffect(() => {
    const flush = () => {
      if (dirty() && activeRef.current) {
        background(save("auto"))
      }
    }
    const onVisibility = () => {
      if (document.visibilityState === "hidden") {
        flush()
      }
    }
    document.addEventListener("visibilitychange", onVisibility)
    window.addEventListener("pagehide", flush)
    return () => {
      document.removeEventListener("visibilitychange", onVisibility)
      window.removeEventListener("pagehide", flush)
    }
  }, [dirty, save])

  const rename = useCallback(
    async (id: string, name: string) => {
      const library = libraryRef.current
      if (!library) {
        return
      }
      try {
        const slot = await library.rename(id, name)
        const current = activeRef.current
        if (slot && current?.id === id) {
          setActive({ ...current, name: slot.name })
        }
        notify(`Renamed to “${name}”.`)
      } catch (error) {
        notify(error instanceof Error ? error.message : String(error), "error")
      }
      await refresh()
    },
    [notify, refresh, setActive],
  )

  const remove = useCallback(
    async (id: string) => {
      const library = libraryRef.current
      if (!library) {
        return
      }
      try {
        await library.remove(id)
        if (activeRef.current?.id === id) {
          setActive(null)
          setStatus("Deleted from the library — save as a new park to keep it")
        }
        notify("Park deleted from this browser.")
      } catch (error) {
        notify(error instanceof Error ? error.message : String(error), "error")
      }
      await refresh()
    },
    [notify, refresh, setActive],
  )

  const readSave = useCallback(async (key: string) => {
    const library = libraryRef.current
    if (!library) {
      throw new StorageError("unavailable", "Browser storage is unavailable.")
    }
    return library.read(key)
  }, [])

  const download = useCallback(
    (name: string, text: string) => {
      try {
        const url = URL.createObjectURL(new Blob([text], { type: "application/json" }))
        const link = document.createElement("a")
        link.href = url
        link.download = saveFileName(name)
        document.body.append(link)
        link.click()
        link.remove()
        window.setTimeout(() => URL.revokeObjectURL(url), 10_000)
        setExportFallback(null)
        notify(`Exported “${name}” as ${saveFileName(name)}.`)
      } catch {
        setExportFallback({ name, text })
        notify(
          "The browser blocked the download. Copy the save text from the park menu instead.",
          "error",
        )
      }
    },
    [notify],
  )

  const exportCurrent = useCallback(() => {
    const document = captureSave()
    if (document) {
      download(activeRef.current?.name ?? "park", document)
    }
  }, [captureSave, download])

  const exportSlot = useCallback(
    async (entry: SlotEntry) => {
      const newest = entry.saves[0]
      if (!newest) {
        return
      }
      try {
        download(entry.slot.name, await readSave(newest.key))
      } catch (error) {
        notify(error instanceof Error ? error.message : String(error), "error")
      }
    },
    [download, notify, readSave],
  )

  /**
   * Validates a save file through zoo-core and adds it to the library as a new slot. Returns
   * the document when it is valid but cannot be stored, so the caller can open it directly.
   */
  const importFile = useCallback(
    async (file: File): Promise<{ stored: boolean; document: string } | null> => {
      if (file.size > MAX_SAVE_BYTES) {
        notify(loadFailureMessage("too_large", ""), "error")
        return null
      }
      let document: string
      try {
        document = await file.text()
      } catch (error) {
        notify(`The file could not be read (${String(error)}).`, "error")
        return null
      }
      const check = validateSave(document)
      if (!check.ok) {
        notify(loadFailureMessage(check.code, check.message), "error")
        return null
      }
      const library = libraryRef.current
      if (!library) {
        return { stored: false, document }
      }
      const name = nameFromFile(file.name)
      try {
        const slot = await library.create(name, document)
        await refresh()
        notify(
          `Imported “${name}” as a new park (${slot.latest ? describeSave(slot.latest) : ""}).`,
        )
        return { stored: true, document }
      } catch (error) {
        notify(error instanceof Error ? error.message : String(error), "error")
        return { stored: false, document }
      }
    },
    [notify, refresh, validateSave],
  )

  const setCadence = useCallback((value: AutosaveCadence) => {
    writeCadence(value)
    setCadenceState(value)
  }, [])

  const touch = useCallback(
    async (id: string) => {
      try {
        await libraryRef.current?.touch(id)
      } catch {
        // Only the Continue ordering depends on it.
      }
      await refresh()
    },
    [refresh],
  )

  return {
    ready,
    available: libraryRef.current !== null,
    storageNotice,
    entries,
    active,
    status,
    conflict,
    exportFallback,
    cadence,
    setCadence,
    refresh,
    markChanged,
    dirty,
    adopt,
    defaultName,
    createSlot,
    save,
    autosaveIfDue,
    rename,
    remove,
    readSave,
    exportCurrent,
    exportSlot,
    importFile,
    touch,
    dismissExportFallback: () => setExportFallback(null),
  }
}

export type ParkLibraryState = ReturnType<typeof useParkLibrary>
