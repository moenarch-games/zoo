// #89 browser acceptance: park library, autosave recovery and portable save files.
//
// Called by capture-park-view-proof.mjs after the existing journeys, against the same production
// preview and real headless Chromium. Every save is a real zoo-core document stored in the real
// IndexedDB library; reloads are real navigations; imports go through the real file input; the
// competing tab is a second real page target sharing the same browser profile. Only the failed
// write is induced, by making IndexedDB `put` throw QuotaExceededError in the page.

import { writeFileSync } from "node:fs"

const START = `[aria-label="Zoo start"]`
const MENU = `[aria-label="Park menu"][role="dialog"]`

/**
 * Picks `scenario` in the open New park picker (#91) and starts it. Returns false when the
 * picker or its Start button is missing.
 */
export async function startPickedScenario({ evaluate, sleep }, scenario) {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    const state = await evaluate(`(() => {
      const card = document.querySelector('.scenario-card[data-scenario=${JSON.stringify(scenario)}]')
      if (!card) return "waiting"
      card.click()
      return "picked"
    })()`)
    if (state === "picked") break
    if (attempt === 49) return false
    await sleep(100)
  }
  await sleep(50)
  return evaluate(`(() => {
    const button = [...document.querySelectorAll('.library-panel button')].find((candidate) =>
      candidate.textContent.startsWith('Start “'))
    if (!button || button.disabled) return false
    button.click()
    return true
  })()`)
}

/** Waits for the park view, then frames the whole park as the pre-#91 journeys expect. */
export async function frameWholePark({ evaluate, sleep }) {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    if (await evaluate(`Boolean(window.__zooParkView?.ready && window.__zooParkView.camera())`)) break
    await sleep(100)
  }
  await evaluate(`[...document.querySelectorAll('button')].find((candidate) =>
    (candidate.getAttribute('aria-label') ?? candidate.textContent ?? '').includes('Reset view'))?.click()`)
  await sleep(150)
}

/**
 * Waits for the start screen (or a startup failure), opens New park, starts `scenario` and,
 * unless `wholePark` is false, resets the scenario's opening focus to the whole-park view.
 */
export async function enterNewPark(context, { scenario = "first-zoo", wholePark = true } = {}) {
  const { evaluate, sleep } = context
  for (let attempt = 0; attempt < 200; attempt += 1) {
    const state = await evaluate(`(() => {
      if (document.querySelector('[aria-label="Park recovery"]')?.textContent.includes('Park could not start')) return "failed"
      const button = [...document.querySelectorAll('${START} button')].find((candidate) => candidate.textContent === 'New park')
      if (!button) return "waiting"
      button.click()
      return "picking"
    })()`)
    if (state === "failed") return state
    if (state === "picking") {
      if (!(await startPickedScenario(context, scenario))) return "no-picker"
      if (wholePark) await frameWholePark(context)
      return "started"
    }
    await sleep(100)
  }
  throw new Error("Timed out waiting for the Zoo start screen")
}

export async function runParkLibraryAcceptance(context) {
  const {
    cdp,
    evaluate,
    assert,
    sleep,
    previewUrl,
    tile,
    drag,
    screenshot,
    connectCdp,
    debuggingPort,
  } = context
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await cdp.send("DOM.enable")

  const waitFor = async (description, expression, attempts = 100, page = evaluate) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await page(expression)) return
      await sleep(100)
    }
    const state = await page(`({
      start: document.querySelector('${START}')?.textContent ?? null,
      menu: document.querySelector('${MENU}')?.textContent ?? null,
      message: document.querySelector('.message')?.textContent ?? null,
      status: document.querySelector('.save-status')?.textContent ?? null,
    })`)
    throw new Error(`#89 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const press = (scope, label, page = evaluate) =>
    page(`(() => {
      const button = [...document.querySelectorAll(${JSON.stringify(`${scope} button`)})].find((candidate) =>
        (candidate.getAttribute('aria-label') ?? candidate.textContent ?? '').startsWith(${JSON.stringify(label)}))
      if (!button || button.disabled) return false
      button.click()
      return true
    })()`)
  const pressIn = async (scope, label, page = evaluate) => {
    assert(await press(scope, label, page), `#89: ${label} control missing`)
    await sleep(150)
  }
  const panelText = (page = evaluate) =>
    page(
      `document.querySelector('${MENU}')?.textContent ?? document.querySelector('${START}')?.textContent ?? ''`,
    )
  const libraryMessage = (page = evaluate) =>
    page(
      `document.querySelector('.library-message')?.textContent ?? document.querySelector('.message')?.textContent ?? ''`,
    )
  const cash = () => evaluate(`document.querySelector('.stat strong')?.textContent ?? ''`)
  const hudClock = (page = evaluate) =>
    page(
      `[...document.querySelectorAll('.stat')].find((stat) => stat.querySelector('span')?.textContent === 'Day')?.querySelector('strong')?.textContent ?? ''`,
    )
  const status = (page = evaluate) =>
    page(`document.querySelector('.save-status')?.textContent ?? ''`)
  const slotNames = () =>
    evaluate(
      `[...document.querySelectorAll('.library-slots > li')].map((item) => item.dataset.slot)`,
    )
  const ready = (page = evaluate) =>
    waitFor(
      "the park renderer",
      `Boolean(window.__zooParkView?.ready && document.querySelector('.park-view[data-renderer="ready"]'))`,
      200,
      page,
    )
  const openMenu = async (page = evaluate) => {
    await pressIn(".speed-controls", "Park menu", page)
    await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 20, page)
  }
  const closeMenu = (page = evaluate) => pressIn(MENU, "Back to park", page)
  const save = async (page = evaluate) => {
    await openMenu(page)
    await pressIn(MENU, "Save", page)
    await waitFor(
      "the save confirmation",
      `document.querySelector('.library-message')?.textContent.startsWith('Saved “') ?? false`,
      50,
      page,
    )
    const text = await libraryMessage(page)
    await closeMenu(page)
    return text
  }
  const navigate = async () => {
    await cdp.send("Page.navigate", { url: previewUrl })
    await waitFor("the start screen", `Boolean(document.querySelector('${START} button'))`, 200)
  }
  const openLoad = async (scope) => {
    await pressIn(scope, "Load park")
    await waitFor(
      "the saved-park list",
      `document.querySelectorAll('.library-slots > li').length > 0`,
      30,
    )
  }
  const loadSlot = async (name) => {
    assert(
      await evaluate(`(() => {
      const item = [...document.querySelectorAll('.library-slots > li')].find((entry) => entry.dataset.slot === ${JSON.stringify(name)})
      const button = [...(item?.querySelectorAll('button') ?? [])].find((candidate) => candidate.textContent === 'Load')
      button?.click()
      return Boolean(button)
    })()`),
      `#89: no Load button for ${name}`,
    )
    await sleep(150)
    // Replacing a park with unsaved progress asks first; this journey discards it.
    if ((await panelText()).includes("not saved yet")) await pressIn(MENU, "Discard changes")
    await ready()
    await waitFor(
      `${name} to open`,
      `document.querySelector('.brand span')?.textContent === ${JSON.stringify(name)}`,
      50,
    )
  }
  const pause = async () => {
    await pressIn(".speed-controls", "Ⅱ")
  }
  const buildPath = async (fromX, toX) => {
    assert(await context.clickButton("Path"), "#89: path tool missing")
    await drag(await tile(fromX, 7), await tile(toX, 7))
  }

  // --- 1. Two different parks survive a reload and resume exactly -------------------------------
  await navigate()
  assert((await enterNewPark(context)) === "started", "#89: start screen did not start a park")
  await ready()
  await pause()
  const parkA = await evaluate(`document.querySelector('.brand span').textContent`)
  await buildPath(3, 7)
  // Canceling New park on unsaved progress keeps the park.
  await openMenu()
  await pressIn(MENU, "New park")
  assert(await startPickedScenario(context, "first-zoo"), "#91: New park picker missing")
  await sleep(150)
  assert(
    (await panelText()).includes("not saved yet"),
    "#89: New park did not warn about unsaved progress",
    await panelText(),
  )
  await pressIn(MENU, "Cancel")
  assert(
    (await evaluate(`document.querySelector('.brand span').textContent`)) === parkA,
    "#89: canceled New park replaced the park",
  )
  await pressIn(MENU, "Back")
  await closeMenu()
  const savedA = await save()
  const cashA = await cash()
  const clockA = await hudClock()

  await openMenu()
  await pressIn(MENU, "New park")
  assert(await startPickedScenario(context, "first-zoo"), "#91: New park picker missing")
  await sleep(150)
  await ready()
  await frameWholePark(context)
  await pause()
  const parkB = await evaluate(`document.querySelector('.brand span').textContent`)
  assert(parkB !== parkA, "#89: second park reused the first slot", { parkA, parkB })
  await buildPath(3, 12)
  await save()
  const cashB = await cash()
  assert(cashA !== cashB, "#89: the two parks are not different", { cashA, cashB })

  await navigate()
  assert(
    (await panelText()).includes(`Continue “${parkB}”`),
    "#89: Continue does not offer the last park",
    await panelText(),
  )
  await screenshot("test-results/park-library-start.png")
  await openLoad(START)
  assert(
    (await slotNames()).includes(parkA) && (await slotNames()).includes(parkB),
    "#89: library lost a park",
    await slotNames(),
  )
  await loadSlot(parkA)
  await pause()
  assert(
    (await cash()) === cashA &&
      (await libraryMessage()).includes(`day ${clockA.replace(" · ", ", ")}`),
    "#89: park A did not resume exactly",
    { cashA, now: await cash(), clockA, message: await libraryMessage() },
  )
  await navigate()
  await pressIn(START, "Continue")
  await ready()
  await pause()
  assert((await cash()) === cashA, "#89: Continue did not resume the last played park", {
    cashA,
    now: await cash(),
  })
  await navigate()
  await openLoad(START)
  await loadSlot(parkB)
  await pause()
  assert((await cash()) === cashB, "#89: park B did not resume exactly", {
    cashB,
    now: await cash(),
  })
  await screenshot("test-results/park-library-resumed.png")

  // --- 2. A failed write keeps the previous save and does not claim success --------------------
  await buildPath(13, 15)
  await evaluate(`(() => {
    window.__zoo89Put = IDBObjectStore.prototype.put
    IDBObjectStore.prototype.put = function () { throw new DOMException("zoo-89 acceptance: quota", "QuotaExceededError") }
  })()`)
  await openMenu()
  await pressIn(MENU, "Save")
  await waitFor(
    "the quota failure",
    `document.querySelector('.library-message')?.textContent.includes('Browser storage is full') ?? false`,
    50,
  )
  assert(
    (await status()).startsWith("Save failed"),
    "#89: failed save claimed success",
    await status(),
  )
  await evaluate(`IDBObjectStore.prototype.put = window.__zoo89Put`)
  await closeMenu()
  await navigate()
  await openLoad(START)
  await loadSlot(parkB)
  await pause()
  assert((await cash()) === cashB, "#89: failed write changed the previous valid save", {
    cashB,
    now: await cash(),
  })

  // --- 3. Export one park and import it as a distinct slot -------------------------------------
  await openMenu()
  await openLoad(MENU)
  await evaluate(`(() => {
    window.__zoo89Export = null
    const original = URL.createObjectURL
    URL.createObjectURL = (blob) => { blob.text().then((text) => { window.__zoo89Export = text }); return original(blob) }
  })()`)
  assert(
    await evaluate(`(() => {
    const item = [...document.querySelectorAll('.library-slots > li')].find((entry) => entry.dataset.slot === ${JSON.stringify(parkA)})
    const button = [...(item?.querySelectorAll('button') ?? [])].find((candidate) => candidate.textContent === 'Export')
    button?.click()
    return Boolean(button)
  })()`),
    "#89: Export control missing",
  )
  await waitFor("the exported save", `typeof window.__zoo89Export === 'string'`, 30)
  const exported = await evaluate(`window.__zoo89Export`)
  assert(JSON.parse(exported).format === "zoo-save", "#89: export is not a zoo-core save")
  const filePath = `${process.cwd()}/test-results/park-a.zoo-save.json`
  writeFileSync(filePath, exported)
  const setFile = async (path) => {
    const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
    const { nodeId } = await cdp.send("DOM.querySelector", {
      nodeId: root.nodeId,
      selector: 'input[type="file"]',
    })
    await cdp.send("DOM.setFileInputFiles", { nodeId, files: [path] })
    await sleep(400)
  }
  const slotsBefore = (await slotNames()).length
  await setFile(filePath)
  await waitFor(
    "the import",
    `document.querySelector('.library-message')?.textContent.startsWith('Imported “park-a”') ?? false`,
    50,
  )
  assert(
    (await slotNames()).length === slotsBefore + 1,
    "#89: import did not add a distinct slot",
    await slotNames(),
  )
  assert((await cash()) === cashB, "#89: import changed the active park")

  // Unsupported, newer, corrupt and oversized files explain themselves and change nothing.
  const document = JSON.parse(exported)
  const rejects = [
    ["corrupt.zoo-save.json", "{ not json", "not valid JSON"],
    [
      "wrong.zoo-save.json",
      JSON.stringify({ ...document, format: "other-game" }),
      "not a Zoo park save",
    ],
    [
      "newer.zoo-save.json",
      JSON.stringify({ ...document, schema_version: 99 }),
      "newer version of Zoo",
    ],
    [
      "damaged.zoo-save.json",
      JSON.stringify({ ...document, park: { ...document.park, cash_cents: "lots" } }),
      "damaged or was edited",
    ],
    ["huge.zoo-save.json", `${exported}${" ".repeat(4 * 1024 * 1024)}`, "4 MB save limit"],
  ]
  for (const [name, text, expected] of rejects) {
    const path = `${process.cwd()}/test-results/${name}`
    writeFileSync(path, text)
    await setFile(path)
    await waitFor(
      `the ${name} rejection`,
      `document.querySelector('.library-message')?.textContent.includes(${JSON.stringify(expected)}) ?? false`,
      50,
    )
    assert((await slotNames()).length === slotsBefore + 1, `#89: rejected ${name} added a slot`)
    assert((await cash()) === cashB, `#89: rejected ${name} changed the active park`)
  }
  await loadSlot("park-a")
  await pause()
  assert((await cash()) === cashA, "#89: imported park does not match its export", {
    cashA,
    now: await cash(),
  })

  // Canceled and confirmed delete.
  await openMenu()
  await openLoad(MENU)
  const deleteControl = (label) =>
    evaluate(`(() => {
    const item = [...document.querySelectorAll('.library-slots > li')].find((entry) => entry.dataset.slot === ${JSON.stringify(parkB)})
    const button = [...(item?.querySelectorAll('button') ?? [])].find((candidate) => candidate.textContent === ${JSON.stringify(label)})
    button?.click()
    return Boolean(button)
  })()`)
  assert(await deleteControl("Delete"), "#89: Delete control missing")
  assert(await deleteControl("Keep park"), "#89: delete confirmation missing")
  assert((await slotNames()).includes(parkB), "#89: canceled delete removed the park")
  assert(await deleteControl("Delete"), "#89: Delete control missing")
  assert(await deleteControl("Delete park"), "#89: delete confirmation missing")
  await waitFor(
    "the deleted park to leave the list",
    `![...document.querySelectorAll('.library-slots > li')].some((entry) => entry.dataset.slot === ${JSON.stringify(parkB)})`,
    30,
  )
  await pressIn(MENU, "Back")
  await closeMenu()

  // --- 4. Autosave preserves in-progress work and reports the saved tick ----------------------
  await openMenu()
  await pressIn(MENU, "Settings")
  await evaluate(`(() => {
    const select = document.querySelector('${MENU} select')
    const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set
    setter.call(select, '30')
    select.dispatchEvent(new Event('change', {bubbles: true}))
  })()`)
  await pressIn(MENU, "Back")
  await closeMenu()
  await buildPath(12, 14)
  await pressIn(".speed-controls", "4×")
  await waitFor(
    "an autosave",
    `document.querySelector('.save-status')?.textContent.startsWith('Autosaved') ?? false`,
    200,
  )
  await pause()
  const autosaved = await status()
  const [, day, time, tick] = autosaved.match(/day (\d+) · (\d\d:\d\d) \(tick (\d+)\)/) ?? []
  assert(day && Number(tick) > 0, "#89: autosave did not report its tick", autosaved)
  await navigate()
  assert(
    (await panelText()).includes(`day ${day} · ${time} (tick ${tick})`),
    "#89: Continue does not offer the autosave at its reported tick",
    { autosaved, panel: await panelText() },
  )
  await pressIn(START, "Continue")
  await ready()
  await pause()
  assert(
    (await libraryMessage()).includes(`day ${day}, ${time}`) &&
      (await libraryMessage()).includes("autosave"),
    "#89: recovery did not report the autosave it opened",
    await libraryMessage(),
  )
  // The autosave is later than the imported park's manual save, so the unsaved path survived.
  assert(`${day} · ${time}` !== clockA, "#89: autosave recovery fell back to the manual save", {
    clockA,
  })

  // --- 5. A competing tab's newer save is detected, never overwritten silently -----------------
  const created = await cdp.send("Target.createTarget", { url: previewUrl })
  let second = null
  try {
    const targets = await (await fetch(`http://127.0.0.1:${debuggingPort}/json`)).json()
    const target = targets.find((candidate) => candidate.id === created.targetId)
    second = connectCdp(target.webSocketDebuggerUrl)
    await second.opened
    await second.send("Runtime.enable")
    const evaluateSecond = async (expression) => {
      const response = await second.send("Runtime.evaluate", {
        expression,
        awaitPromise: true,
        returnByValue: true,
      })
      if (response.exceptionDetails) throw new Error(response.exceptionDetails.text)
      return response.result.value
    }
    await waitFor(
      "the second tab's start screen",
      `Boolean(document.querySelector('${START} button'))`,
      200,
      evaluateSecond,
    )
    await pressIn(START, "Continue", evaluateSecond)
    await ready(evaluateSecond)
    await pressIn(".speed-controls", "Ⅱ", evaluateSecond)
    await save(evaluateSecond)
  } finally {
    second?.close()
    await cdp.send("Target.closeTarget", { targetId: created.targetId })
  }
  await cdp.send("Page.bringToFront")
  await buildPath(15, 16)
  await openMenu()
  await pressIn(MENU, "Save")
  await waitFor(
    "the conflict",
    `document.querySelector('${MENU}')?.textContent.includes('was saved in another tab') ?? false`,
    50,
  )
  assert(
    (await status()).includes("conflict"),
    "#89: conflict did not pause autosave",
    await status(),
  )
  await screenshot("test-results/park-library-conflict.png")
  await pressIn(MENU, "Save this tab as a new park")
  await evaluate(`document.querySelector('${MENU} form button[type="submit"]').click()`)
  await waitFor(
    "the conflict copy",
    `document.querySelector('.brand span')?.textContent.endsWith('(this tab)') ?? false`,
    50,
  )
  assert(
    !(await panelText()).includes("was saved in another tab"),
    "#89: conflict persisted after saving a copy",
  )
  await closeMenu()

  // --- 6. Keyboard and phone-sized touch complete the same lifecycle ---------------------------
  await evaluate(`document.activeElement?.blur()`)
  const key = async (value) => {
    const code = value === "Enter" ? 13 : 27
    await cdp.send("Input.dispatchKeyEvent", {
      type: "keyDown",
      key: value,
      code: value,
      windowsVirtualKeyCode: code,
      text: value === "Enter" ? "\r" : undefined,
    })
    await cdp.send("Input.dispatchKeyEvent", {
      type: "keyUp",
      key: value,
      code: value,
      windowsVirtualKeyCode: code,
    })
    await sleep(150)
  }
  await evaluate(`document.querySelector('.park-menu-button').focus()`)
  await key("Enter")
  await waitFor("the keyboard-opened menu", `Boolean(document.querySelector('${MENU}'))`, 20)
  assert(
    await evaluate(`document.activeElement?.closest('${MENU}') !== null`),
    "#89: park menu did not take keyboard focus",
  )
  await key("Enter")
  await waitFor(
    "the keyboard save",
    `document.querySelector('.library-message')?.textContent.startsWith('Saved “') ?? false`,
    50,
  )
  await key("Escape")
  assert(
    !(await evaluate(`Boolean(document.querySelector('${MENU}'))`)),
    "#89: Escape did not close the park menu",
  )

  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: 390,
    height: 844,
    deviceScaleFactor: 2,
    mobile: true,
  })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  const tap = async (scope, label) => {
    const point = await evaluate(`(() => {
      const button = [...document.querySelectorAll(${JSON.stringify(`${scope} button`)})].find((candidate) =>
        (candidate.getAttribute('aria-label') ?? candidate.textContent ?? '').startsWith(${JSON.stringify(label)}))
      if (!button) return null
      button.scrollIntoView({block: 'center'})
      const rect = button.getBoundingClientRect()
      return {x: rect.left + rect.width / 2, y: rect.top + rect.height / 2}
    })()`)
    assert(point, `#89: ${label} is not tappable`)
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [{ x: point.x, y: point.y, id: 0 }],
    })
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
    await sleep(250)
  }
  await tap(".speed-controls", "Park menu")
  await waitFor("the phone park menu", `Boolean(document.querySelector('${MENU}'))`, 20)
  assert(
    await evaluate(`document.querySelector('${MENU}').getBoundingClientRect().right <= innerWidth`),
    "#89: park menu overflows the phone",
  )
  await screenshot("test-results/park-library-phone.png")
  await tap(MENU, "Save")
  await waitFor(
    "the phone save",
    `document.querySelector('.library-message')?.textContent.startsWith('Saved “') ?? false`,
    50,
  )
  await tap(MENU, "Load park")
  await waitFor(
    "the phone saved-park list",
    `document.querySelectorAll('.library-slots > li').length > 0`,
    30,
  )
  await tap(MENU, "Back")
  await tap(MENU, "Back to park")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })

  console.log(
    `Park library acceptance passed: two parks (${parkA}, ${parkB}) resumed after reloads, ${savedA}; a quota-failed write kept the previous save; export/import added a distinct slot; corrupt, foreign, newer, damaged and oversized files were refused without mutation; autosave recovered "${autosaved}"; a second tab's newer save raised a conflict; keyboard and phone touch completed save/load.`,
  )
}
