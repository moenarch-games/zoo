// Browser park library (#89): named park slots in IndexedDB, each holding one manual save and a
// bounded rotating autosave history. The documents are zoo-core `zoo-save` envelopes (#88); this
// module only stores them and never interprets park rules. Summaries (day, time, tick) are read
// from the document zoo-core produced so the library reports exactly what was written.
//
// Every write is one IndexedDB transaction over the slot and its save record, guarded by the
// slot's revision: a tab that loaded revision n can only write while the slot is still at n, so a
// newer save from another tab is detected instead of silently overwritten. An aborted transaction
// (quota denial, a closed tab, a revision conflict) leaves the previous records untouched.

const DB_NAME = "zoo-park-library"
const DB_VERSION = 1
const SLOTS = "slots"
const SAVES = "saves"
/** Autosaves kept per park; the oldest is dropped in the same transaction as the newest write. */
export const AUTOSAVE_HISTORY = 3
/** zoo-core's `MAX_SAVE_BYTES`: larger imports are refused before they are read. */
export const MAX_SAVE_BYTES = 4 * 1024 * 1024

export type SaveKind = "manual" | "auto"

export type SaveSummary = {
  key: string
  slotId: string
  kind: SaveKind
  savedAt: number
  day: number
  minute: number
  tick: number
  bytes: number
}

export type SlotMeta = {
  id: string
  name: string
  scenario: string
  createdAt: number
  playedAt: number
  /** Bumped by every save; the conflict guard for competing tabs. */
  revision: number
  autoSeq: number
  latest: SaveSummary | null
}

export type SlotEntry = {
  slot: SlotMeta
  /** Newest first: the manual save and the autosave history. */
  saves: SaveSummary[]
}

type SaveRecord = SaveSummary & {
  document: string
}

export type StorageFailure = "unavailable" | "quota" | "conflict" | "missing" | "failed"

export class StorageError extends Error {
  constructor(
    readonly kind: StorageFailure,
    message: string,
    readonly current: SlotMeta | null = null,
  ) {
    super(message)
  }
}

/** Day, time and simulation tick as recorded in a zoo-core save document. */
export function summarizeDocument(document: string) {
  const value = JSON.parse(document) as {
    content?: { ruleset_id?: string }
    park?: {
      day?: number
      minute_of_day?: number
      elapsed_minutes?: number
      scenario?: { title?: string }
    }
  }
  return {
    scenario: value.park?.scenario?.title ?? value.content?.ruleset_id ?? "unknown",
    day: value.park?.day ?? 0,
    minute: value.park?.minute_of_day ?? 0,
    tick: value.park?.elapsed_minutes ?? 0,
  }
}

export function describeSave(save: Pick<SaveSummary, "day" | "minute" | "tick">) {
  const time = `${String(Math.floor(save.minute / 60)).padStart(2, "0")}:${String(save.minute % 60).padStart(2, "0")}`
  return `day ${save.day} · ${time} (tick ${save.tick})`
}

function classify(error: unknown, fallback: string): StorageError {
  if (error instanceof StorageError) {
    return error
  }
  const name = error instanceof DOMException ? error.name : ""
  const text = error instanceof Error ? error.message : String(error)
  if (name === "QuotaExceededError" || /quota/i.test(text)) {
    return new StorageError(
      "quota",
      "Browser storage is full. The previous save is unchanged; export this park to a file to keep it.",
    )
  }
  return new StorageError(
    "failed",
    `${fallback} (${name || text}). The previous save is unchanged.`,
  )
}

function request<T>(req: IDBRequest<T>) {
  return new Promise<T>((resolve, reject) => {
    req.onsuccess = () => resolve(req.result)
    req.onerror = () => reject(req.error)
  })
}

export class ParkLibrary {
  private constructor(private readonly db: IDBDatabase) {}

  static async open(): Promise<ParkLibrary> {
    try {
      if (typeof indexedDB === "undefined") {
        throw new Error("IndexedDB is not available")
      }
      const opening = indexedDB.open(DB_NAME, DB_VERSION)
      opening.onupgradeneeded = () => {
        const db = opening.result
        db.createObjectStore(SLOTS, { keyPath: "id" })
        db.createObjectStore(SAVES, { keyPath: "key" }).createIndex("slotId", "slotId")
      }
      const db = await new Promise<IDBDatabase>((resolve, reject) => {
        opening.onsuccess = () => resolve(opening.result)
        opening.onerror = () => reject(opening.error)
        opening.onblocked = () => reject(new Error("another Zoo tab is upgrading the library"))
      })
      // A newer build in another tab may upgrade the schema; let it, and stop writing here.
      db.onversionchange = () => db.close()
      try {
        background(navigator.storage?.persist?.() ?? Promise.resolve())
      } catch {
        // Persistence is a best-effort request; the library works without it.
      }
      return new ParkLibrary(db)
    } catch (error) {
      throw new StorageError(
        "unavailable",
        `Browser storage is unavailable (${error instanceof Error ? error.message : String(error)}). Export parks to files to keep them.`,
      )
    }
  }

  private transaction<T>(
    mode: IDBTransactionMode,
    work: (
      slots: IDBObjectStore,
      saves: IDBObjectStore,
      abort: (error: StorageError) => void,
    ) => Promise<T>,
    failure: string,
  ): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      let tx: IDBTransaction
      try {
        tx = this.db.transaction([SLOTS, SAVES], mode)
      } catch (error) {
        reject(classify(error, failure))
        return
      }
      let result: T
      let aborted: StorageError | null = null
      const abort = (error: StorageError) => {
        aborted = error
        tx.abort()
      }
      tx.oncomplete = () => resolve(result)
      tx.onabort = () => reject(aborted ?? classify(tx.error, failure))
      work(tx.objectStore(SLOTS), tx.objectStore(SAVES), abort).then(
        (value) => {
          result = value
        },
        (error: unknown) => {
          if (aborted === null) {
            aborted = classify(error, failure)
            try {
              tx.abort()
            } catch {
              // Already finished or aborted.
            }
          }
        },
      )
    })
  }

  async list(): Promise<SlotEntry[]> {
    return this.transaction(
      "readonly",
      async (slots, saves) => {
        const [metas, records] = await Promise.all([
          request(slots.getAll() as IDBRequest<SlotMeta[]>),
          request(saves.getAll() as IDBRequest<SaveRecord[]>),
        ])
        return metas
          .map((slot) => ({
            slot,
            saves: records
              .filter((record) => record.slotId === slot.id)
              .map(({ document: _document, ...summary }) => summary)
              .sort((a, b) => b.savedAt - a.savedAt || b.tick - a.tick),
          }))
          .sort((a, b) => b.slot.playedAt - a.slot.playedAt)
      },
      "Could not read the park library",
    )
  }

  async read(key: string): Promise<string> {
    return this.transaction(
      "readonly",
      async (_slots, saves) => {
        const record = (await request(saves.get(key))) as SaveRecord | undefined
        if (!record) {
          throw new StorageError(
            "missing",
            "That save no longer exists; it may have been deleted in another tab.",
          )
        }
        return record.document
      },
      "Could not read the save",
    )
  }

  /** A new slot whose first manual save is `document`. */
  async create(name: string, document: string): Promise<SlotMeta> {
    const now = Date.now()
    const summary = summarizeDocument(document)
    const id =
      typeof crypto.randomUUID === "function" ? crypto.randomUUID() : `${now}-${Math.random()}`
    const save: SaveRecord = {
      key: `${id}/manual`,
      slotId: id,
      kind: "manual",
      savedAt: now,
      day: summary.day,
      minute: summary.minute,
      tick: summary.tick,
      bytes: document.length,
      document,
    }
    const { document: _document, ...latest } = save
    const slot: SlotMeta = {
      id,
      name,
      scenario: summary.scenario,
      createdAt: now,
      playedAt: now,
      revision: 1,
      autoSeq: 0,
      latest,
    }
    return this.transaction(
      "readwrite",
      async (slots, saves) => {
        await request(saves.put(save))
        await request(slots.put(slot))
        return slot
      },
      "Could not create the park",
    )
  }

  /**
   * Writes a manual save or the next autosave for `slotId`, provided the slot is still at
   * `expectedRevision` (pass `null` to overwrite deliberately after a conflict).
   */
  async write(
    slotId: string,
    kind: SaveKind,
    document: string,
    expectedRevision: number | null,
  ): Promise<SlotMeta> {
    const now = Date.now()
    const summary = summarizeDocument(document)
    return this.transaction(
      "readwrite",
      async (slots, saves, abort) => {
        const slot = (await request(slots.get(slotId))) as SlotMeta | undefined
        if (!slot) {
          throw new StorageError(
            "missing",
            "This park was deleted from the library, possibly in another tab.",
          )
        }
        if (expectedRevision !== null && slot.revision !== expectedRevision) {
          abort(
            new StorageError(
              "conflict",
              `“${slot.name}” was saved in another tab${slot.latest ? ` at ${describeSave(slot.latest)}` : ""}.`,
              slot,
            ),
          )
          return slot
        }
        const autoSeq = kind === "auto" ? slot.autoSeq + 1 : slot.autoSeq
        const record: SaveRecord = {
          key: kind === "auto" ? `${slotId}/auto/${autoSeq}` : `${slotId}/manual`,
          slotId,
          kind,
          savedAt: now,
          day: summary.day,
          minute: summary.minute,
          tick: summary.tick,
          bytes: document.length,
          document,
        }
        await request(saves.put(record))
        if (kind === "auto" && autoSeq > AUTOSAVE_HISTORY) {
          await request(saves.delete(`${slotId}/auto/${autoSeq - AUTOSAVE_HISTORY}`))
        }
        const { document: _document, ...latest } = record
        const next: SlotMeta = {
          ...slot,
          playedAt: now,
          revision: slot.revision + 1,
          autoSeq,
          latest,
        }
        await request(slots.put(next))
        return next
      },
      kind === "auto" ? "Autosave failed" : "Save failed",
    )
  }

  async touch(slotId: string): Promise<SlotMeta | null> {
    return this.update(slotId, (slot) => ({ ...slot, playedAt: Date.now() }))
  }

  async rename(slotId: string, name: string): Promise<SlotMeta | null> {
    return this.update(slotId, (slot) => ({ ...slot, name }))
  }

  private update(slotId: string, change: (slot: SlotMeta) => SlotMeta) {
    return this.transaction(
      "readwrite",
      async (slots) => {
        const slot = (await request(slots.get(slotId))) as SlotMeta | undefined
        if (!slot) {
          return null
        }
        const next = change(slot)
        await request(slots.put(next))
        return next
      },
      "Could not update the park",
    )
  }

  async remove(slotId: string): Promise<void> {
    return this.transaction(
      "readwrite",
      async (slots, saves) => {
        const keys = await request(saves.index("slotId").getAllKeys(slotId))
        await Promise.all(keys.map((key) => request(saves.delete(key))))
        await request(slots.delete(slotId))
      },
      "Could not delete the park",
    )
  }
}

/** Actionable text for zoo-core `load_save` failure codes. */
export function loadFailureMessage(code: string | undefined, message: string) {
  const messages: Record<string, string> = {
    too_large: `This file is larger than the ${MAX_SAVE_BYTES / 1024 / 1024} MB save limit, so it cannot be a Zoo park.`,
    malformed:
      "This file is not readable as a Zoo save (it is not valid JSON). Choose a file exported from Zoo.",
    unsupported_format: "This file is not a Zoo park save. Choose a file exported from Zoo.",
    unsupported_version: message.includes("newer")
      ? `This save comes from a newer version of Zoo (${message}). Update the game to open it.`
      : `This save uses a schema this game does not know (${message}).`,
    invalid_save: `This save is damaged or was edited by hand and cannot be opened (${message}).`,
    invalid_park: `This save is damaged or was edited by hand and cannot be opened (${message}).`,
    invalid_content: `This save's species and facility content is invalid (${message}).`,
  }
  return (code && messages[code]) || `This save could not be opened (${message}).`
}

export type AutosaveCadence = 0 | 30 | 60 | 180 | 1440

export const AUTOSAVE_CADENCES: { value: AutosaveCadence; label: string }[] = [
  { value: 0, label: "Off" },
  { value: 30, label: "Every 30 game minutes" },
  { value: 60, label: "Every game hour" },
  { value: 180, label: "Every 3 game hours" },
  { value: 1440, label: "Every game day" },
]

const CADENCE_KEY = "zoo.autosaveCadence"
export const DEFAULT_CADENCE: AutosaveCadence = 60

export function readCadence(): AutosaveCadence {
  try {
    const value = Number(localStorage.getItem(CADENCE_KEY))
    return AUTOSAVE_CADENCES.some((option) => option.value === value) &&
      localStorage.getItem(CADENCE_KEY) !== null
      ? (value as AutosaveCadence)
      : DEFAULT_CADENCE
  } catch {
    return DEFAULT_CADENCE
  }
}

export function writeCadence(value: AutosaveCadence) {
  try {
    localStorage.setItem(CADENCE_KEY, String(value))
  } catch {
    // A per-browser preference; the in-memory value still applies this session.
  }
}

export function saveFileName(name: string) {
  const base =
    name
      .trim()
      .replace(/[^\p{L}\p{N}_-]+/gu, "-")
      .replace(/^-+|-+$/g, "") || "park"
  return `${base}.zoo-save.json`
}

export function nameFromFile(fileName: string) {
  return fileName.replace(/\.zoo-save\.json$|\.json$/i, "").trim() || "Imported park"
}

/** Runs library work whose failures it already reports; anything unexpected reaches the console. */
export function background(work: Promise<unknown>) {
  work.catch((error: unknown) => console.error("Park library task failed", error))
}
