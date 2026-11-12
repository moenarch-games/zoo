// #103 browser acceptance: visitors leave dissatisfied for an explained reason, the player fixes
// the cause and visits recover, and one group is followed through several habitats, a service
// stop and its exit.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. The player opens the authored three-habitat park (crates/zoo-core/tests/fixtures/
// visitor-park.json, written by zoo-core's own test) through the real file import, with the
// guests already inside staged thirsty. Closing the drink stand from its inspector leaves them
// without a drink: the workspace's "Visits ended" report counts them as "Needs unmet" and names
// the state behind it. Reopening the stand brings sales back and satisfied exits with them. Then
// a newly arrived group is picked from the Guests table and its member's inspector is watched
// until the guest walks out. Park time and every value come from zoo-core through the inspector
// and workspace.

import { readFileSync, writeFileSync } from "node:fs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const WORKSPACE = `section[aria-label="Management workspace"]`
const VISIT = `.side-panel section.guest-visit`
const FIXTURE = new URL("../../../crates/zoo-core/tests/fixtures/visitor-park.json", import.meta.url)
/** Park minutes each comparison window lasts. */
const WINDOW = 240

function stagedSave() {
  const document = JSON.parse(readFileSync(FIXTURE, "utf8"))
  for (const guest of document.park.guests) {
    guest.thirst = 70
  }
  return JSON.stringify(document)
}

export async function runVisitorItinerariesAcceptance(context) {
  const { cdp, evaluate, assert, sleep, screenshot, clickButton, frame } = context
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({
      message: document.querySelector('.message')?.textContent ?? null,
      panel: document.querySelector('.side-panel .window-title span')?.textContent ?? null,
      visit: document.querySelector('${VISIT}')?.textContent ?? null,
      exits: document.querySelector('.visit-outcomes')?.textContent ?? null,
    })`)
    throw new Error(`#103 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const tick = () => evaluate(`JSON.parse(window.__zooDiagnostics.exportText()).session.clock.tick`)
  const workspaceOpen = () => evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`)
  const openWorkspace = async (tab) => {
    if (!(await workspaceOpen())) {
      assert(await clickButton("Management workspace"), "#103: Manage control missing")
      await waitFor("the workspace", `Boolean(document.querySelector('${WORKSPACE}'))`, 30)
    }
    await evaluate(`[...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith(${JSON.stringify(tab)}))?.click()`)
    await frame()
  }
  const closeWorkspace = async () => {
    if (await workspaceOpen()) {
      await evaluate(`document.querySelector('${WORKSPACE} .window-title button')?.click()`)
      await frame()
    }
  }
  /** Today's exit counts by reason, read from the workspace's "Visits ended" report. */
  const exits = async () => {
    await openWorkspace("Today")
    return evaluate(`Object.fromEntries([...document.querySelectorAll('.visit-outcomes [data-exit-reason]')]
      .map((row) => [row.dataset.exitReason, Number(row.dataset.exitCount)]))`)
  }
  const recentExits = () =>
    evaluate(`[...document.querySelectorAll('.visit-recent li')].map((row) => ({ reason: row.dataset.exitReason, text: row.textContent }))`)
  const run = async (minutes) => {
    const start = await tick()
    assert(await clickButton("4×"), "#103: speed control missing")
    await waitFor(`${minutes} park minutes`, `JSON.parse(window.__zooDiagnostics.exportText()).session.clock.tick >= ${start + minutes}`, 1500)
    assert(await clickButton("Ⅱ"), "#103: pause control missing")
    await frame()
  }
  /** Presses a stand-inspector button with the mouse. */
  const press = async (text) => {
    const box = await evaluate(`(() => {
      const button = [...document.querySelectorAll('.side-panel button')].find((b) => b.textContent === ${JSON.stringify(text)})
      if (!button || button.disabled) return null
      button.scrollIntoView({ block: "center" })
      const r = button.getBoundingClientRect()
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
    })()`)
    assert(box, `#103: the inspector has no usable ${text} control`)
    await frame()
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: box.x, y: box.y, button: "none" })
    await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: box.x, y: box.y, button: "left", buttons: 1, clickCount: 1 })
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: box.x, y: box.y, button: "left", buttons: 0, clickCount: 1 })
    await frame()
  }
  const inspectDrinkStand = async () => {
    await openWorkspace("Facilities")
    assert(
      await evaluate(`(() => {
        const row = [...document.querySelectorAll('${WORKSPACE} tbody tr')].find((r) => r.textContent.startsWith('Drink stand #'))
        row?.click()
        return Boolean(row)
      })()`),
      "#103: the facilities table does not list the drink stand",
    )
    await frame()
    await closeWorkspace()
    await waitFor("the drink stand inspector", `(document.querySelector('.side-panel .window-title span')?.textContent ?? '').startsWith('Drink stand #')`, 30)
  }

  // --- Open the authored park through the real file import -------------------------------------
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  const name = "visitor-park"
  const path = `${process.cwd()}/test-results/${name}.zoo-save.json`
  writeFileSync(path, stagedSave())
  assert(await clickButton("Park menu"), "#103: park menu control missing")
  await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
  await sleep(150)
  const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[type="file"]' })
  assert(nodeId, "#103: the park file input is missing")
  await cdp.send("DOM.setFileInputFiles", { nodeId, files: [path] })
  await waitFor("the import", `[...document.querySelectorAll('.library-slots > li')].some((item) => item.dataset.slot === ${JSON.stringify(name)})`, 60)
  await evaluate(`(() => {
    const item = [...document.querySelectorAll('.library-slots > li')].find((entry) => entry.dataset.slot === ${JSON.stringify(name)})
    ;[...(item?.querySelectorAll('button') ?? [])].find((b) => b.textContent === 'Load')?.click()
  })()`)
  await sleep(200)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'Discard changes')?.click()`)
  await waitFor("the park", `(document.querySelector('.message')?.textContent ?? '').startsWith('Park loaded')`, 100)
  await waitFor("the renderer", `Boolean(window.__zooParkView?.ready && window.__zooParkView.camera())`, 200)
  assert(await clickButton("Ⅱ"), "#103: pause control missing")

  // --- Dissatisfaction: thirsty guests and a closed drink stand ---------------------------------
  await inspectDrinkStand()
  await press("Close stand")
  await waitFor("the closed stand", `document.querySelector('.side-panel [data-stand-status]')?.dataset.standStatus === 'closed'`, 30)
  const closedAt = await exits()
  await run(WINDOW)
  const closed = await exits()
  const unmetWhileClosed = closed.needs_unmet - closedAt.needs_unmet
  assert(unmetWhileClosed > 0, "#103: no guest left with an unmet need while the drink stand was closed", { closedAt, closed })
  await evaluate(`[...document.querySelectorAll('${WORKSPACE} details.summary-row > summary')].find((s) => s.textContent.startsWith('Visits ended'))?.click()`)
  await frame()
  const explained = (await recentExits()).find((row) => row.reason === "needs_unmet" && /thirsty and the park could not help/.test(row.text))
  assert(explained, "#103: the report does not say why guests left", await recentExits())
  await screenshot("test-results/visits-dissatisfied-desktop.png")

  // --- Recovery: reopen the stand; drinks sell and guests leave satisfied ----------------------
  await inspectDrinkStand()
  await press("Open stand")
  await waitFor("the open stand", `document.querySelector('.side-panel [data-stand-status]')?.dataset.standStatus === 'open'`, 30)
  const salesBefore = await evaluate(`Number(document.querySelector('.side-panel [data-stand-sales]')?.dataset.standSales ?? NaN)`)
  await run(WINDOW)
  const salesAfter = await evaluate(`Number(document.querySelector('.side-panel [data-stand-sales]')?.dataset.standSales ?? NaN)`)
  const reopened = await exits()
  const unmetWhileOpen = reopened.needs_unmet - closed.needs_unmet
  assert(salesAfter > salesBefore, "#103: the reopened drink stand sold nothing", { salesBefore, salesAfter })
  assert(unmetWhileOpen < unmetWhileClosed, "#103: reopening the stand did not cut unmet needs", { unmetWhileClosed, unmetWhileOpen })
  assert(reopened.satisfied > closed.satisfied, "#103: no guest left satisfied after the fix", { closed, reopened })
  await screenshot("test-results/visits-recovered-desktop.png")

  // --- Follow a newly arrived group through its visit -------------------------------------------
  await openWorkspace("Guests")
  const newestGuest = () =>
    evaluate(`Math.max(0, ...[...document.querySelectorAll('${WORKSPACE} tbody tr')].map((r) => Number(r.querySelector('td')?.textContent ?? 0)))`)
  let followed = null
  for (let attempt = 0; attempt < 12 && followed === null; attempt += 1) {
    const before = await newestGuest()
    assert(await clickButton("4×"), "#103: speed control missing")
    await waitFor("a new group", `Math.max(0, ...[...document.querySelectorAll('${WORKSPACE} tbody tr')].map((r) => Number(r.querySelector('td')?.textContent ?? 0))) > ${before}`, 600)
    assert(await clickButton("Ⅱ"), "#103: pause control missing")
    await frame()
    const id = await newestGuest()
    await evaluate(`[...document.querySelectorAll('${WORKSPACE} tbody tr')].find((r) => r.querySelector('td')?.textContent === '${id}')?.click()`)
    await frame()
    await closeWorkspace()
    await waitFor("the guest inspector", `Boolean(document.querySelector('${VISIT}[data-guest-id="${id}"]'))`, 30)
    const groupSize = await evaluate(`Number(document.querySelector('${VISIT}').dataset.guestGroupSize)`)
    if (groupSize >= 2) {
      followed = id
    } else {
      await openWorkspace("Guests")
    }
  }
  assert(followed !== null, "#103: no group of several people arrived")
  await screenshot("test-results/visits-group-arrived-desktop.png")
  const visited = new Set()
  let errand = false
  let waited = false
  let exit = ""
  let group = ""
  assert(await clickButton("4×"), "#103: speed control missing")
  for (let attempt = 0; attempt < 1500; attempt += 1) {
    const state = await evaluate(`(() => {
      const visit = document.querySelector('${VISIT}[data-guest-id="${followed}"]')
      return visit ? { ...visit.dataset } : null
    })()`)
    if (state === null) break
    group = state.guestGroup
    for (const habitat of state.guestVisited.split(",").filter(Boolean)) visited.add(habitat)
    errand ||= state.guestErrand !== ""
    waited ||= state.guestWaiting === "true"
    if (state.guestExit) {
      if (!exit) await screenshot("test-results/visits-group-leaving-desktop.png")
      exit = state.guestExit
    }
    await sleep(100)
  }
  assert(await clickButton("Ⅱ"), "#103: pause control missing")
  await frame()
  assert(!(await evaluate(`Boolean(document.querySelector('${VISIT}[data-guest-id="${followed}"]'))`)), "#103: the followed guest never left")
  assert(visited.size >= 2, "#103: the followed guest saw fewer than two habitats", { visited: [...visited] })
  assert(errand, "#103: the followed guest made no service stop")
  assert(exit, "#103: the followed guest left without a reason")
  await openWorkspace("Today")
  await evaluate(`[...document.querySelectorAll('${WORKSPACE} details.summary-row > summary')].find((s) => s.textContent.startsWith('Visits ended') && !s.parentElement.open)?.click()`)
  await frame()
  const note = (await recentExits()).find((row) => row.text.startsWith(`Guest #${followed} `))
  assert(note && note.reason === exit, "#103: the report does not list the followed guest's exit", { exit, recent: await recentExits() })
  await screenshot("test-results/visits-report-desktop.png")

  // --- Phone: the same report fits a phone ----------------------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(400)
  await frame()
  await openWorkspace("Today")
  assert(await evaluate(`Boolean(document.querySelector('.visit-outcomes'))`), "#103: the visits report is missing on a phone")
  await screenshot("test-results/visits-report-phone.png")
  await closeWorkspace()
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  console.log(
    `#103 visitor-itineraries acceptance passed: with the drink stand closed ${unmetWhileClosed} guests left with needs unmet ("${explained.text}"); reopened it sold ${salesAfter - salesBefore} drinks and ${unmetWhileOpen} left unmet; guest #${followed} of group #${group} saw habitats ${[...visited].join(", ")}, made a service stop${waited ? ", waited for its group" : ""} and left (${exit}: "${note.text}")`,
  )
}
