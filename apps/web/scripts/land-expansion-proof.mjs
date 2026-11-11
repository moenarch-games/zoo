// #99 land and expansion browser proof. Called by capture-park-view-proof.mjs in the same
// production preview and headless Chromium. On the unfinished lakeside it compares the three
// authored parcels in the Land panel (a dry paddock the main path reaches, a cheaper half-lake
// shore no path reaches, a locked meadow), sees a path onto the paddock refused with the parcel
// named and nothing spent, plans a path and a habitat on the paddock without spending, selects
// parcels on the map (the quote totals them), buys the paddock, extends access, builds the
// planned path and habitat and stocks and staffs it as the zoo's second operating habitat. Last,
// on a phone-sized touch viewport, it selects a parcel by touch and cancels the selection.
// Prices, statuses, quotes and results are read from Rust's messages and the recorded session.
import { enterNewPark } from "./park-library-proof.mjs"

export async function runLandExpansionAcceptance(context) {
  const { cdp, evaluate, assert, sleep, previewUrl, tile, clickButton, mouse, key, frame, screenshot } =
    context

  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const waitFor = async (description, expression, attempts = 100) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    throw new Error(`#99 acceptance timed out waiting for ${description}: ${await message()}`)
  }
  const expectMessage = (text) =>
    waitFor(
      `the message "${text}"`,
      `document.querySelector('.message')?.textContent.includes(${JSON.stringify(text)}) ?? false`,
    )
  const press = async (selector) => {
    const pressed = await evaluate(`(() => {
      const button = document.querySelector(${JSON.stringify(selector)})
      if (!button || button.disabled) return false
      button.click()
      return true
    })()`)
    assert(pressed, `#99: control ${selector} missing or disabled`)
    await frame()
  }
  const down = (point) => mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
  const up = async (point) => {
    await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
    await frame()
  }
  const drag = async (from, to) => {
    const a = await tile(...from)
    const b = await tile(...to)
    await mouse("mouseMoved", a)
    await down(a)
    for (let step = 1; step <= 8; step += 1) {
      const t = step / 8
      await mouse("mouseMoved", { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }, { button: "left", buttons: 1 })
    }
    await frame()
    await up(b)
    await sleep(150)
  }
  const tap = async (x, y) => {
    const point = await tile(x, y)
    await mouse("mouseMoved", point)
    await down(point)
    await up(point)
    await sleep(150)
  }
  const session = async () => JSON.parse(await evaluate(`window.__zooDiagnostics.exportText()`)).session
  const lastCommand = async (kind) => (await session()).commands.filter((entry) => entry.kind === kind).at(-1)
  const cashText = () => evaluate(`document.querySelector('.stat strong')?.textContent ?? ''`)
  const parcelRow = (id) =>
    evaluate(`(() => {
      const button = document.querySelector('[data-parcel="${id}"]')
      return button ? { text: button.textContent, pressed: button.getAttribute('aria-pressed') === 'true', disabled: button.disabled } : null
    })()`)
  const quoteTotal = () => evaluate(`document.querySelector('[data-land-total]')?.dataset.landTotal ?? null`)
  const settleCamera = async () => {
    let last = ""
    for (let attempt = 0; attempt < 60; attempt += 1) {
      const now = JSON.stringify(await evaluate("window.__zooParkView.camera()"))
      if (now === last) return
      last = now
      await sleep(150)
    }
  }

  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await cdp.send("Page.navigate", { url: previewUrl })
  assert(
    (await enterNewPark({ evaluate, sleep }, { scenario: "partly-prepared" })) === "started",
    "#99: the unfinished lakeside did not start",
  )
  await waitFor("the park renderer", `Boolean(window.__zooParkView?.ready && window.__zooParkView.nodeCount() > 500)`, 200)
  assert(await clickButton("Ⅱ"), "#99: pause control missing")
  await key("Home")
  await settleCamera()
  assert(await evaluate(`window.__zooParkView.parkFootprintInFrame()`), "#99: the whole park is not framed")
  // Unbought land is marked in the scene before any tool is chosen.
  assert(await evaluate(`window.__zooParkView.staticNode("land:east-paddock:20:10") !== null`), "#99: unbought land is not marked")

  // --- Compare the parcels --------------------------------------------------------------------
  assert(await clickButton("Land"), "#99: land tool missing")
  await waitFor("the land panel", `Boolean(document.querySelector('[aria-label="Land"] [data-parcel]'))`)
  const paddock = await parcelRow("east-paddock")
  const shore = await parcelRow("lakeshore")
  const meadow = await parcelRow("south-meadow")
  assert(paddock && shore && meadow, "#99: the land panel does not list every parcel", { paddock, shore, meadow })
  assert(paddock.text.includes("$9,500") && paddock.text.includes("For sale") && paddock.text.includes("path reaches it") && !paddock.text.includes("water"),
    "#99: the paddock's trade-off is not shown", paddock.text)
  assert(shore.text.includes("$5,000") && shore.text.includes("16 water") && shore.text.includes("no path yet"),
    "#99: the lakeshore's trade-off is not shown", shore.text)
  assert(meadow.text.includes("Locked: opens once a habitat has animals"), "#99: the meadow's restriction is not shown", meadow.text)
  await screenshot("test-results/land-compare-desktop.png")

  // --- Building on unbought land is refused, nothing spent ------------------------------------
  const cashBefore = await cashText()
  assert(await clickButton("Path"), "#99: path tool missing")
  await drag([18, 8], [21, 8])
  await expectMessage("Outside owned land: buy East Paddock ($9500) first")
  assert((await cashText()) === cashBefore, "#99: a refused path spent money", { cashBefore, after: await cashText() })

  // --- Plan without spending -------------------------------------------------------------------
  await press("[data-plan-toggle]")
  await drag([18, 8], [22, 8])
  await expectMessage("Added to the plan")
  assert(await clickButton("Draw habitat"), "#99: habitat tool missing")
  await waitFor("the plan toggle in the habitat tools", `document.querySelector('[data-plan-toggle]')?.getAttribute('aria-pressed') === 'true'`)
  await drag([19, 9], [22, 12])
  await expectMessage("Added to the plan")
  await waitFor("two plan items", `document.querySelectorAll('[data-plan-item]').length === 2`)
  const planText = await evaluate(`document.querySelector('[aria-label="Expansion plan"]')?.textContent ?? ''`)
  assert(planText.includes("Outside owned land: buy East Paddock") && planText.includes("land $9,500 (East Paddock)"),
    "#99: the plan does not show the land it needs", planText)
  assert(await evaluate(`window.__zooParkView.staticNode("plan:2:20:10") !== null`), "#99: the planned habitat is not drawn")
  assert((await cashText()) === cashBefore, "#99: planning spent money", { cashBefore, after: await cashText() })
  assert(!(await session()).commands.some((entry) => entry.kind === "build_path_stroke" && entry.result.ok),
    "#99: planning built something")
  await press("[data-plan-toggle]")
  await screenshot("test-results/land-plan-desktop.png")

  // --- Select parcels on the map and buy one --------------------------------------------------
  assert(await clickButton("Land"), "#99: land tool missing")
  await tap(20, 11)
  await waitFor("the paddock selection", `document.querySelector('[data-parcel="east-paddock"]')?.getAttribute('aria-pressed') === 'true'`)
  await waitFor("the paddock quote", `document.querySelector('[data-land-total]')?.dataset.landTotal === '950000'`)
  await tap(18, 6)
  await waitFor("both parcels quoted", `document.querySelector('[data-land-total]')?.dataset.landTotal === '1450000'`)
  await tap(18, 6)
  await waitFor("the lakeshore deselected", `document.querySelector('[data-land-total]')?.dataset.landTotal === '950000'`)
  assert(await evaluate(`window.__zooParkView.staticNode("land:east-paddock:20:10")?.color === "#f6d36f"`), "#99: the selection is not highlighted")
  await screenshot("test-results/land-selected-desktop.png")
  await press("[data-land-buy]")
  await expectMessage("Bought East Paddock · $9500")
  const bought = await lastCommand("buy_land")
  assert(bought?.result.ok && JSON.stringify(bought.args.parcels) === '["east-paddock"]', "#99: the purchase was not recorded", bought)
  await waitFor("the paddock owned", `document.querySelector('[data-parcel="east-paddock"]')?.disabled === true`)
  assert((await quoteTotal()) === null, "#99: the bought parcel stayed selected")

  // --- Extend access and build the plan -------------------------------------------------------
  assert(await clickButton("Path"), "#99: path tool missing")
  await drag([12, 8], [13, 8])
  await expectMessage("Path built · 2 new public path tiles")
  // The planned path is buildable on the bought land; the planned habitat waits for its path.
  await waitFor("the planned path to become buildable", `Boolean(document.querySelector('[data-plan-item="1"].valid'))`)
  await press('[data-plan-item="1"] button')
  await expectMessage("Path built · 5 new public path tiles")
  await waitFor("the planned habitat to become buildable", `Boolean(document.querySelector('[data-plan-item="2"].valid'))`)
  await press('[data-plan-item="2"] button')
  await expectMessage("Habitat #2 fenced")
  await waitFor("the plan to empty", `!document.querySelector('[aria-label="Expansion plan"]')`)

  // --- Staff and stock it: the zoo's second operating habitat ---------------------------------
  assert(await clickButton("Inspect"), "#99: inspect tool missing")
  await tap(2, 7)
  await waitFor("the depot inspector", `document.querySelector('.window-title span')?.textContent === 'Central operations depot'`)
  assert(await clickButton("Hire keeper"), "#99: Hire keeper missing")
  await expectMessage("Keeper #1 hired")
  assert(await clickButton("feed crates"), "#99: feed purchase missing")
  assert(await clickButton("Zones"), "#99: zones tool missing")
  await drag([12, 6], [23, 10])
  await expectMessage("Work zone #1 created")
  assert(await clickButton("Assign keeper"), "#99: Assign keeper missing")
  await expectMessage("Keeper #1 assigned to work zone #1")
  assert(await clickButton("Inspect"), "#99: inspect tool missing")
  await tap(20, 10)
  await waitFor("habitat #2's inspector", `document.querySelector('.window-title span')?.textContent === 'Habitat #2'`)
  for (let count = 0; count < 2; count += 1) {
    assert(await clickButton("Capybara"), "#99: capybara adoption missing", await message())
    await frame()
  }
  const adoptions = (await session()).commands.filter((entry) => entry.kind === "adopt" && entry.args.habitat_id === 2)
  assert(adoptions.length === 2 && adoptions.every((entry) => entry.result.ok), "#99: the new habitat was not stocked", adoptions)
  await key("Home")
  await settleCamera()
  assert(await evaluate(`window.__zooParkView.parkFootprintInFrame()`), "#99: expansion changed the park framing")
  await screenshot("test-results/land-expanded-desktop.png")

  // --- Phone: select a parcel by touch and cancel ---------------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(600)
  await evaluate(`document.querySelector('.window-title button')?.click()`)
  assert(await clickButton("Land"), "#99: land tool missing on phone")
  await key("Home")
  await settleCamera()
  const target = await tile(19, 1)
  const onCanvas = await evaluate(`document.elementFromPoint(${target.x}, ${target.y})?.tagName === 'CANVAS'`)
  assert(target.visible && onCanvas, "#99: the phone parcel tile is not on the park canvas", {
    target,
    hit: await evaluate(`document.elementFromPoint(${target.x}, ${target.y})?.outerHTML.slice(0, 160) ?? null`),
  })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: target.x, y: target.y, id: 0 }] })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
  await frame()
  await waitFor("the touch selection", `document.querySelector('[data-land-total]')?.dataset.landTotal === '500000'`)
  const sheet = await evaluate(`(() => {
    const panel = document.querySelector('.side-panel')?.getBoundingClientRect()
    const buy = document.querySelector('[data-land-buy]')?.getBoundingClientRect()
    const map = document.querySelector('.park-canvas')?.getBoundingClientRect()
    return panel && buy && map
      ? { top: panel.top, bottom: panel.bottom, buyTop: buy.top, buyBottom: buy.bottom, mapTop: map.top, mapHeight: map.height }
      : null
  })()`)
  assert(
    sheet && sheet.top > sheet.mapTop + sheet.mapHeight * 0.4 && sheet.buyTop >= sheet.top && sheet.buyBottom <= sheet.bottom + 1,
    "#99: the phone land sheet hides Buy or covers most of the map",
    sheet,
  )
  await screenshot("test-results/land-phone.png")
  assert(await clickButton("Clear"), "#99: clear control missing on phone")
  await waitFor("the cleared selection", `!document.querySelector('[data-land-total]')`)
  assert((await lastCommand("buy_land")).args.parcels[0] === "east-paddock", "#99: cancelling bought something")
  assert(await clickButton("Close land"), "#99: close control missing on phone")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })

  console.log(
    "Land expansion acceptance passed: compared the paddock, lakeshore and locked meadow, saw a path onto unbought land refused with nothing spent, planned a path and habitat without spending, selected parcels on the map with a totalled quote, bought the paddock, extended access, built the plan and stocked and staffed a second operating habitat, and selected and cancelled a parcel by touch on a phone.",
  )
}
