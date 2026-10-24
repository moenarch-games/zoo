// #94 browser acceptance: the management workspace, actionable alerts and overlays, on desktop
// with the mouse and keyboard and on a phone with touch.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. The player opens a troubled park (the schema 4 save fixture, staged with an empty
// feed store, a broken-down stand nobody can repair and four guests crowding a viewpoint for
// two, because the current guest flow rarely overfills a viewpoint on its own) through the real
// file import. Every fix starts from the alert: Show focuses the cause and opens its inspector,
// the suggested action runs the command or picks the tool, and the alert must disappear once
// zoo-core no longer sees the condition. An isolated stand is built and reconnected by real
// canvas picking. Overlays, sortable/searchable tables, drill-downs and empty states are read
// from the DOM zoo-core's snapshot renders.

import { readFileSync, writeFileSync } from "node:fs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const WORKSPACE = `section[aria-label="Management workspace"]`
const FIXTURE = new URL("../../../crates/zoo-core/tests/fixtures/save-v4.json", import.meta.url)

/** The fixture park, staged with three problems for the alerts to lead the player to. */
function stagedSave() {
  const document = JSON.parse(readFileSync(FIXTURE, "utf8"))
  const park = document.park
  const now = (park.day - 1) * 1440 + park.minute_of_day
  park.feed_crates = 0
  // Food stand #1 has broken down, its repair request is open and no mechanic is employed.
  park.mechanics = []
  park.concessions[0].condition = 15
  park.maintenance = [
    { id: park.next_maintenance_id, concession_id: 1, created_minute: now, assigned_mechanic_id: null },
  ]
  park.next_maintenance_id += 1
  // Four guests watch habitat #2's one capybara from the path tile north of it, a spot for two.
  for (const guest of park.guests.slice(0, 4)) {
    Object.assign(guest, {
      state: "viewing",
      target_habitat: 2,
      x: 8,
      y: 7,
      route: [],
      route_index: 0,
      viewing_minutes: 20,
      arrival_steps: 0,
      visited_habitats: [2],
    })
  }
  return JSON.stringify(document)
}

export async function runManagementAcceptance(context) {
  const { cdp, evaluate, assert, sleep, screenshot, clickButton, frame } = context
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({
      alerts: [...document.querySelectorAll('.alert-item')].map((item) => item.dataset.code + ': ' + item.querySelector('p')?.textContent),
      message: document.querySelector('.message')?.textContent ?? null,
      panel: document.querySelector('.side-panel .window-title span')?.textContent ?? null,
    })`)
    throw new Error(`#94 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const panelTitle = () => evaluate(`document.querySelector('.side-panel .window-title span')?.textContent ?? ''`)
  const alertCodes = (scope = WORKSPACE) =>
    evaluate(`[...document.querySelectorAll('${scope} .alert-item')].map((item) => item.dataset.code)`)
  const project = (x, y) => evaluate(`window.__zooParkView.projectWorld([${x + 0.5}, 0, ${y + 0.5}])`)
  const camera = () => evaluate(`window.__zooParkView.camera()`)

  const openTab = async (tab) => {
    if (!(await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`))) {
      assert(await clickButton("Management workspace"), "#94: Manage control missing")
      await waitFor("the workspace", `Boolean(document.querySelector('${WORKSPACE}'))`, 30)
    }
    assert(
      await evaluate(`(() => {
        const button = [...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith(${JSON.stringify(tab)}))
        button?.click()
        return Boolean(button)
      })()`),
      `#94: ${tab} tab missing`,
    )
    await frame()
  }

  /** Presses a button inside the alert with `code` in the workspace's Alerts tab. */
  const alertButton = async (code, label) => {
    await openTab("Alerts")
    const pressed = await evaluate(`(() => {
      const item = document.querySelector('${WORKSPACE} .alert-item[data-code=${JSON.stringify(code)}]')
      const button = [...(item?.querySelectorAll('button') ?? [])].find((b) => b.textContent.startsWith(${JSON.stringify(label)}))
      button?.click()
      return Boolean(button)
    })()`)
    assert(pressed, `#94: the ${code} alert has no ${label} button`, await alertCodes())
    await frame()
  }

  const setOverlay = async (kind) => {
    await evaluate(`(() => {
      const select = document.querySelector('select[aria-label="Overlay"]')
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(select, ${JSON.stringify(kind)})
      select.dispatchEvent(new Event('change', { bubbles: true }))
    })()`)
    await frame()
  }
  const legendCount = (level) =>
    evaluate(`(() => {
      const swatch = document.querySelector('.overlay-legend .legend-swatch.${level}')
      return Number(swatch?.parentElement?.querySelector('strong')?.textContent ?? -1)
    })()`)

  const pointerInput = (phone) => {
    const touch = (type, points) =>
      cdp.send("Input.dispatchTouchEvent", { type, touchPoints: points.map(([x, y], id) => ({ x, y, id })) })
    const mouse = (type, point, extra = {}) =>
      cdp.send("Input.dispatchMouseEvent", { type, x: point.x, y: point.y, button: "none", ...extra })
    const reachable = async (label, point) => {
      assert(point.visible, `${label}: target is off screen`, point)
      assert(
        await evaluate(`document.elementFromPoint(${point.x}, ${point.y})?.classList.contains('park-canvas') ?? false`),
        `${label}: the HUD covers the target`,
        await evaluate(`document.elementFromPoint(${point.x}, ${point.y})?.outerHTML.slice(0, 200) ?? null`),
      )
    }
    const clearMap = async () => {
      // Alert jumps move the camera; frame the whole park again before picking tiles.
      await evaluate(`[...document.querySelectorAll("button")].find((b) => b.getAttribute("aria-label") === "Reset view")?.click()`)
      await frame()
      // A phone inspector sheet covers the lower map; close it before touching the park.
      if (phone) {
        await evaluate(`document.querySelector('.side-panel .window-title button')?.click()`)
        await frame()
        await evaluate(`(() => { const guide = document.querySelector('details[aria-label="Park guide"]'); if (guide) guide.open = false })()`)
        await frame()
      }
    }
    return {
      async drag(label, from, to) {
        await clearMap()
        const a = await project(from.x, from.y)
        const b = await project(to.x, to.y)
        await reachable(`${label} start`, a)
        await reachable(`${label} end`, b)
        if (phone) {
          await touch("touchStart", [[a.x, a.y]])
          for (let step = 1; step <= 10; step += 1) {
            const t = step / 10
            await touch("touchMove", [[a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t]])
          }
          await touch("touchEnd", [])
        } else {
          await mouse("mouseMoved", a)
          await mouse("mousePressed", a, { button: "left", buttons: 1, clickCount: 1 })
          for (let step = 1; step <= 10; step += 1) {
            const t = step / 10
            await mouse("mouseMoved", { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }, { button: "left", buttons: 1 })
          }
          await mouse("mouseReleased", b, { button: "left", buttons: 0, clickCount: 1 })
        }
        await frame()
      },
      async tap(label, at) {
        await clearMap()
        const point = await project(at.x, at.y)
        await reachable(label, point)
        if (phone) {
          await touch("touchStart", [[point.x, point.y]])
          await touch("touchEnd", [])
        } else {
          await mouse("mouseMoved", point)
          await mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
          await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
        }
        await frame()
      },
    }
  }

  /** Imports the staged park through the real park menu and opens it, paused. */
  const openStagedPark = async (label) => {
    const name = `troubled-${label}`
    const path = `${process.cwd()}/test-results/${name}.zoo-save.json`
    writeFileSync(path, stagedSave())
    assert(await clickButton("Park menu"), "#94: park menu control missing")
    await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
    await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
    await sleep(150)
    const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
    const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[type="file"]' })
    assert(nodeId, "#94: the park file input is missing")
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
    assert(await clickButton("Ⅱ"), "#94: pause control missing")
    assert(await clickButton("Reset view"), "#94: Reset view missing")
    await frame()
  }

  const walk = async (phone) => {
    const label = phone ? "phone" : "desktop"
    const input = pointerInput(phone)
    await openStagedPark(label)

    // The biggest problems are visible at a glance in the top bar.
    const badge = await evaluate(`document.querySelector('.alerts-button')?.getAttribute('aria-label') ?? ''`)
    assert(/Alerts: [3-9]/.test(badge), `${label}: the alert count is not shown`, badge)
    assert(
      await evaluate(`document.querySelector('.alerts-button')?.classList.contains('critical') ?? false`),
      `${label}: critical alerts are not marked`,
    )
    await openTab("Alerts")
    const codes = await alertCodes()
    for (const code of ["feed_depot_empty", "facility_failed", "viewpoint_crowded"]) {
      assert(codes.includes(code), `${label}: ${code} alert missing`, codes)
    }
    // Severity has a text label and an icon, not only a colour.
    assert(
      await evaluate(`[...document.querySelectorAll('${WORKSPACE} .alert-item.critical .alert-severity')].every((badge) => badge.getAttribute('aria-label') === 'Critical' && badge.textContent.trim() !== '')`),
      `${label}: severity is shown by colour alone`,
    )
    if (phone) {
      const box = await evaluate(`(() => { const r = document.querySelector('${WORKSPACE}').getBoundingClientRect(); return { left: r.left, right: r.right, bottom: r.bottom, width: innerWidth, height: innerHeight } })()`)
      assert(box.left >= 0 && box.right <= box.width && box.bottom <= box.height, `${label}: the drawer leaves the screen`, box)
      assert(
        !(await evaluate(`Boolean(document.querySelector('.side-panel')?.offsetParent)`)),
        `${label}: the inspector sheet stays under the drawer`,
      )
    }
    await screenshot(`test-results/management-alerts-${label}.png`)

    // 1. Empty feed depot: Show opens the depot and frames it; the action buys feed.
    const before = await camera()
    await alertButton("feed_depot_empty", "Show")
    assert((await panelTitle()) === "Central operations depot", `${label}: Show did not open the depot`, await panelTitle())
    await sleep(400)
    assert(JSON.stringify(await camera()) !== JSON.stringify(before), `${label}: Show did not move the camera`)
    if (phone) {
      assert(!(await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`)), `${label}: the drawer covers the opened inspector`)
    }
    await alertButton("feed_depot_empty", "Buy")
    assert((await message()).includes("feed"), `${label}: buying feed failed`, await message())
    assert(!(await alertCodes()).includes("feed_depot_empty"), `${label}: the feed alert stayed after buying feed`, await alertCodes())

    // 2. Broken-down stand: Show opens the stand; the suggested mechanic is hired.
    await alertButton("facility_failed", "Show")
    assert((await panelTitle()).startsWith("Food stand #1"), `${label}: Show did not open the stand`, await panelTitle())
    await alertButton("facility_failed", "Hire mechanic")
    assert((await message()).toLowerCase().includes("mechanic"), `${label}: hiring failed`, await message())
    await setOverlay("facilities")
    assert((await legendCount("bad")) >= 1, `${label}: the facilities overlay does not mark the broken stand`)

    // 3. Isolated destination: a stand on a stub path the entrance does not reach.
    if (phone) await evaluate(`document.querySelector('${WORKSPACE} .window-title button')?.click()`)
    assert(await clickButton("Path"), `${label}: path tool missing`)
    await input.drag("stub path", { x: 13, y: 11 }, { x: 16, y: 11 })
    assert((await message()).toLowerCase().includes("path"), `${label}: the stub path was not built`, await message())
    assert(await clickButton("Food stand ·"), `${label}: food stand tool missing`)
    assert(
      await evaluate(`document.querySelector(".toolbar .tool.active")?.textContent.includes("Food stand") ?? false`),
      `${label}: the food stand tool is not active`,
      await evaluate(`document.querySelector(".toolbar .tool.active")?.textContent ?? null`),
    )
    await input.tap("isolated stand", { x: 14, y: 12 })
    assert((await message()).includes("built"), `${label}: the stand was not built`, await message())
    await setOverlay("access")
    assert((await legendCount("bad")) >= 1, `${label}: the access overlay does not mark the cut-off stand`)
    await screenshot(`test-results/management-isolated-before-${label}.png`)
    await alertButton("destination_isolated", "Build path here")
    assert(
      await evaluate(`[...document.querySelectorAll('.toolbar .tool')].find((b) => b.textContent.includes('Path'))?.classList.contains('active') ?? false`),
      `${label}: the alert action did not pick the path tool`,
    )
    // On a phone the legend shares the top of the map; the player hides it while drawing.
    if (phone) await setOverlay("")
    await input.drag("connecting path", { x: 12, y: 7 }, { x: 12, y: 11 })
    await input.drag("connecting path", { x: 12, y: 11 }, { x: 13, y: 11 })
    await setOverlay("access")
    await openTab("Alerts")
    assert(!(await alertCodes()).includes("destination_isolated"), `${label}: the isolated alert stayed after connecting`, await alertCodes())
    assert((await legendCount("bad")) === 0, `${label}: the access overlay still marks a cut-off tile`)
    await screenshot(`test-results/management-isolated-after-${label}.png`)

    // 4. Crowded viewpoint: Show frames habitat #2; the viewing overlay marks the spot.
    await alertButton("viewpoint_crowded", "Show")
    assert((await panelTitle()) === "Habitat #2", `${label}: Show did not open habitat #2`, await panelTitle())
    await setOverlay("viewing")
    assert((await legendCount("bad")) >= 1, `${label}: the viewing overlay does not mark the crowd`)
    await screenshot(`test-results/management-crowded-before-${label}.png`)
    assert(await clickButton("4×"), `${label}: speed control missing`)
    await openTab("Alerts")
    await waitFor(`${label} crowd to move on`, `![...document.querySelectorAll('${WORKSPACE} .alert-item')].some((item) => item.dataset.code === 'viewpoint_crowded')`, 400)
    await waitFor(`${label} stand repair`, `![...document.querySelectorAll('${WORKSPACE} .alert-item')].some((item) => item.dataset.code === 'facility_failed')`, 600)
    assert(await clickButton("Ⅱ"), `${label}: pause control missing`)
    await screenshot(`test-results/management-resolved-${label}.png`)
    await setOverlay("")

    // Drill down from a number to its contributors, and search/sort the views.
    await openTab("Today")
    assert(
      await evaluate(`(() => {
        const row = [...document.querySelectorAll('${WORKSPACE} details.summary-row')].find((d) => d.textContent.startsWith('Animal welfare'))
        if (!row) return false
        row.open = true
        return [...row.querySelectorAll('.drill-list button')].some((b) => b.textContent.startsWith('Habitat #1'))
      })()`),
      `${label}: welfare does not drill down to its habitats`,
    )
    await openTab("Habitats")
    assert(
      await evaluate(`document.querySelectorAll('${WORKSPACE} tbody tr').length === 2`),
      `${label}: the habitats table does not list both habitats`,
    )
    // Numeric columns sort largest first on the first press.
    await evaluate(`[...document.querySelectorAll('${WORKSPACE} th button')].find((b) => b.textContent.startsWith('#'))?.click()`)
    await frame()
    assert(
      (await evaluate(`document.querySelector('${WORKSPACE} tbody tr td')?.textContent`)) === "2",
      `${label}: sorting by # descending does not put habitat #2 first`,
    )
    await openTab("Guests")
    await evaluate(`(() => {
      const input = document.querySelector('${WORKSPACE} input[type="search"]')
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, 'no such guest')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })()`)
    await frame()
    assert(
      (await evaluate(`document.querySelector('${WORKSPACE} .empty-state')?.textContent ?? ''`)).includes("match"),
      `${label}: an empty search has no empty state`,
    )
    await openTab("Facilities")
    if (phone) {
      // A row tap focuses the stand and hands the screen back to the map and its inspector.
      await evaluate(`document.querySelector('${WORKSPACE} tbody tr')?.click()`)
    } else {
      // Keyboard: focus a row and press Enter.
      await evaluate(`document.querySelector('${WORKSPACE} tbody tr')?.focus()`)
      await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter" })
      await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Enter", code: "Enter" })
    }
    await frame()
    assert((await panelTitle()).startsWith("Food stand #1"), `${label}: a facility row does not open its stand`, await panelTitle())
    await screenshot(`test-results/management-workspace-${label}.png`)
    await evaluate(`document.querySelector('${WORKSPACE} .window-title button')?.click()`)
  }

  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await walk(false)
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await walk(true)
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  console.log(
    "#94 management acceptance passed: alerts locate and fix an empty feed depot, a broken stand, an isolated stand and a crowded viewpoint; overlays, drill-downs, sortable/searchable tables and empty states on desktop and phone",
  )
}
