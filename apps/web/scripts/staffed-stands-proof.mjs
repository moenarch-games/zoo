// #102 browser acceptance: a busy, understaffed food stand, its cause fixed, and the improved
// throughput, on desktop with the mouse and on a phone with touch.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. The player opens a busy park through the real file import: the current save fixture,
// staged with a lunch crowd of hungry guests by the food stand, which has one vendor (the
// ordinary guest flow rarely brings this many hungry guests at once). The stand inspector shows
// zoo-core's full queue and the guests it turns away; the player posts two more vendors with the
// inspector's own controls, and the stand then sells faster over an equal stretch of park time.
// Park time and every count are read from zoo-core: the diagnostics clock and the snapshot the
// inspector renders.

import { readFileSync, writeFileSync } from "node:fs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const WORKSPACE = `section[aria-label="Management workspace"]`
const FIXTURE = new URL("../../../crates/zoo-core/tests/fixtures/save-v13.json", import.meta.url)
const CROWD = 30
/** Park minutes each throughput window lasts. */
const WINDOW = 40

/** The fixture park with a lunch crowd at a one-vendor food stand in good repair. */
function stagedSave() {
  const document = JSON.parse(readFileSync(FIXTURE, "utf8"))
  const park = document.park
  const food = park.concessions.find((stand) => stand.definition.key === "food")
  food.condition = 100
  park.maintenance = park.maintenance.filter((task) => task.concession_id !== food.id)
  food.stand.vendors = 1
  food.stand.stock = food.definition.stand.stock_capacity
  food.stand.delivery_due = null
  // A mechanic on the path by the depot keeps the busy stand from wearing out mid-proof (#57).
  if (park.mechanics.length === 0) {
    park.mechanics.push({
      id: park.next_mechanic_id,
      x: park.depot.x,
      y: park.depot.y + 1,
      target_maintenance_id: null,
      repairs_completed: 0,
    })
    park.next_mechanic_id += 1
  }
  const counter = { x: food.x, y: food.y - 1 }
  const template = park.guests.find((guest) => guest.visit === null && guest.state !== "arriving")
  for (let offset = 0; offset < CROWD; offset += 1) {
    park.guests.push({
      ...structuredClone(template),
      id: park.next_guest_id + offset,
      x: counter.x,
      y: counter.y,
      state: "walking_to_habitat",
      route: [],
      route_index: 0,
      hunger: 60,
      thirst: 0,
      bladder: 0,
      energy: 100,
      happiness: 80,
      bought_food: false,
      unmet_need: null,
      next_need_check: 0,
      cash_cents: 2000,
    })
  }
  park.next_guest_id += CROWD
  return { text: JSON.stringify(document), standId: food.id }
}

export async function runStaffedStandsAcceptance(context) {
  const { cdp, evaluate, assert, sleep, screenshot, clickButton, frame } = context
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({
      message: document.querySelector('.message')?.textContent ?? null,
      panel: document.querySelector('.side-panel .window-title span')?.textContent ?? null,
      stand: [...document.querySelectorAll('.side-panel .stand-card dd')].map((dd) => dd.textContent),
    })`)
    throw new Error(`#102 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const data = (name) =>
    evaluate(`Number(document.querySelector('.side-panel [data-stand-${name}]')?.dataset.stand${name[0].toUpperCase()}${name.slice(1)} ?? NaN)`)
  const status = () => evaluate(`document.querySelector('.side-panel [data-stand-status]')?.dataset.standStatus ?? ''`)
  const tick = () => evaluate(`JSON.parse(window.__zooDiagnostics.exportText()).session.clock.tick`)
  const panelTitle = () => evaluate(`document.querySelector('.side-panel .window-title span')?.textContent ?? ''`)

  /** Presses an inspector button with real pointer input: the mouse, or a finger on a phone. */
  const press = async (label, phone) => {
    const box = await evaluate(`(() => {
      const button = document.querySelector('.side-panel button[aria-label=${JSON.stringify(label)}]')
      if (!button || button.disabled) return null
      button.scrollIntoView({ block: "center" })
      const r = button.getBoundingClientRect()
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
    })()`)
    assert(box, `#102: the inspector has no usable ${label} control`)
    await frame()
    const hit = await evaluate(`document.elementFromPoint(${box.x}, ${box.y})?.getAttribute('aria-label') ?? null`)
    assert(hit === label, `#102: ${label} is covered`, hit)
    if (phone) {
      await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: box.x, y: box.y, id: 0 }] })
      await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
    } else {
      await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: box.x, y: box.y, button: "none" })
      await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: box.x, y: box.y, button: "left", buttons: 1, clickCount: 1 })
      await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: box.x, y: box.y, button: "left", buttons: 0, clickCount: 1 })
    }
    await frame()
  }

  /** Runs the park at 4× for `minutes` park minutes; returns the stand's sales in that time. */
  const salesOver = async (minutes) => {
    const start = await tick()
    const sold = await data("sales")
    const lost = await data("lost")
    assert(await clickButton("4×"), "#102: speed control missing")
    await waitFor(`${minutes} park minutes`, `JSON.parse(window.__zooDiagnostics.exportText()).session.clock.tick >= ${start + minutes}`, 1200)
    assert(await clickButton("Ⅱ"), "#102: pause control missing")
    await frame()
    const elapsed = (await tick()) - start
    return { sales: (await data("sales")) - sold, lost: (await data("lost")) - lost, elapsed }
  }

  /** Imports the staged park through the real park menu, opens it paused, and inspects the stand. */
  const openBusyPark = async (label) => {
    const name = `lunch-crowd-${label}`
    const path = `${process.cwd()}/test-results/${name}.zoo-save.json`
    const { text, standId } = stagedSave()
    writeFileSync(path, text)
    assert(await clickButton("Park menu"), "#102: park menu control missing")
    await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
    await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
    await sleep(150)
    const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
    const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[type="file"]' })
    assert(nodeId, "#102: the park file input is missing")
    await cdp.send("DOM.setFileInputFiles", { nodeId, files: [path] })
    await waitFor(
      `${label} import`,
      `[...document.querySelectorAll('.library-slots > li')].some((item) => item.dataset.slot === ${JSON.stringify(name)})`,
      60,
    )
    await evaluate(`(() => {
      const item = [...document.querySelectorAll('.library-slots > li')].find((entry) => entry.dataset.slot === ${JSON.stringify(name)})
      ;[...(item?.querySelectorAll('button') ?? [])].find((b) => b.textContent === 'Load')?.click()
    })()`)
    await sleep(200)
    await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'Discard changes')?.click()`)
    await waitFor(`${label} park`, `(document.querySelector('.message')?.textContent ?? '').startsWith('Park loaded')`, 100)
    await waitFor(`${label} renderer`, `Boolean(window.__zooParkView?.ready && window.__zooParkView.camera())`, 200)
    assert(await clickButton("Ⅱ"), "#102: pause control missing")
    // The facilities table opens the stand's inspector.
    if (!(await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`))) {
      assert(await clickButton("Management workspace"), "#102: Manage control missing")
      await waitFor("the workspace", `Boolean(document.querySelector('${WORKSPACE}'))`, 30)
    }
    await evaluate(`[...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith('Facilities'))?.click()`)
    await frame()
    assert(
      await evaluate(`(() => {
        const row = [...document.querySelectorAll('${WORKSPACE} tbody tr')].find((r) => r.textContent.startsWith('Food stand #${standId}'))
        row?.click()
        return Boolean(row)
      })()`),
      "#102: the facilities table does not list the food stand",
    )
    await frame()
    if (await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`)) {
      await evaluate(`document.querySelector('${WORKSPACE} .window-title button')?.click()`)
      await frame()
    }
    await waitFor("the stand inspector", `(document.querySelector('.side-panel .window-title span')?.textContent ?? '').startsWith('Food stand #${standId}')`, 30)
    assert((await data("vendors")) === 1, "#102: the staged stand does not start with one vendor", await data("vendors"))
  }

  // --- Desktop: a busy, understaffed stand ------------------------------------------------------
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await openBusyPark("desktop")
  const capacityBefore = await data("capacity")
  // The crowd reaches the counter: a long queue forms and zoo-core turns guests away.
  assert(await clickButton("4×"), "#102: speed control missing")
  await waitFor(
    "a long queue turning guests away",
    `(() => {
      const queue = document.querySelector('.side-panel [data-stand-queue]')
      const lost = Number(document.querySelector('.side-panel [data-stand-lost]')?.dataset.standLost ?? 0)
      return Number(queue?.dataset.standQueue ?? 0) >= 3 && lost > 0
    })()`,
    900,
  )
  assert(await clickButton("Ⅱ"), "#102: pause control missing")
  await frame()
  assert((await status()) === "open", "#102: the busy stand is not open", await status())
  const lostText = await evaluate(`document.querySelector('.side-panel [data-stand-lost]')?.textContent ?? ''`)
  assert(/queue full|gave up waiting/.test(lostText), "#102: the inspector does not say why sales were lost", lostText)
  await screenshot("test-results/stands-busy-desktop.png")
  const before = await salesOver(WINDOW)
  assert(before.lost > 0, "#102: the understaffed stand did not turn guests away", before)

  // --- Fix the cause: post two more vendors with the inspector's controls ----------------------
  // The park is paused, so the stand's speed (it wears with every sale) holds still here.
  const oneVendor = await data("capacity")
  assert(oneVendor > 0 && oneVendor <= capacityBefore, "#102: the stand's hourly capacity is not shown", { capacityBefore, oneVendor })
  await press("Add a vendor", false)
  await press("Add a vendor", false)
  await waitFor("three vendors", `document.querySelector('.side-panel [data-stand-vendors]')?.dataset.standVendors === '3'`, 30)
  assert((await data("capacity")) === oneVendor * 3, "#102: three vendors do not triple the stand's capacity", {
    oneVendor,
    after: await data("capacity"),
  })
  const after = await salesOver(WINDOW)
  assert(
    after.sales * before.elapsed > before.sales * after.elapsed,
    "#102: posting vendors did not raise the stand's throughput",
    { before, after },
  )
  await screenshot("test-results/stands-staffed-desktop.png")

  // --- Phone: the same controls by touch -------------------------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(400)
  await frame()
  assert((await panelTitle()).startsWith("Food stand #"), "#102: the stand inspector closed on the phone", await panelTitle())
  await press("Remove a vendor", true)
  await waitFor("two vendors after a tap", `document.querySelector('.side-panel [data-stand-vendors]')?.dataset.standVendors === '2'`, 30)
  await screenshot("test-results/stands-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  console.log(
    `#102 staffed-stands acceptance passed: a one-vendor stand's queue turned ${before.lost} guests away and sold ${before.sales} in ${before.elapsed} park minutes; with three vendors it sold ${after.sales} in ${after.elapsed} and lost ${after.lost}; a vendor was removed by touch on a phone`,
  )
}
