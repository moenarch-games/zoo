// #95 habitat shapes browser proof. Called by capture-park-view-proof.mjs on a fresh park in the
// same production preview and headless Chromium. It draws an L-shaped habitat as a fence
// outline along grid corners, inspects it, moves its gate onto the leg, expands it, cancels a
// reshape and undoes one, on desktop (mouse drags) and a phone (touch taps). Prices come from
// Rust's outline preview, not hardcoded values.
import { enterNewPark } from "./park-library-proof.mjs"

export async function runHabitatShapesAcceptance(context) {
  const {
    cdp,
    evaluate,
    assert,
    sleep,
    previewUrl,
    project,
    tile,
    clickButton,
    mouse,
    key,
    frame,
    screenshot,
  } = context

  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const status = () => evaluate(`document.querySelector('.outline-status')?.textContent ?? ''`)
  const waitFor = async (description, expression, attempts = 80) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    throw new Error(
      `#95 acceptance timed out waiting for ${description}: ${await message()} | ${await status()}`,
    )
  }
  const cashCents = async () =>
    Math.round(
      Number(
        (await evaluate(`document.querySelector('.stat strong')?.textContent ?? ''`)).replace(
          /[^0-9.-]/g,
          "",
        ),
      ) * 100,
    )
  const statusCents = async () => {
    const match = (await status()).match(/\$([0-9,.]+)/)
    assert(match, "#95: the outline preview shows no price", await status())
    return Math.round(Number(match[1].replaceAll(",", "")) * 100)
  }
  /** The screen point of grid corner (x, y): the north-west corner of tile (x, y). */
  const corner = (x, y) => project([x, 0, y])
  const press = (point) =>
    mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
  const release = async (point) => {
    await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
    await frame()
  }
  const moveHeld = async (from, to) => {
    for (let step = 1; step <= 10; step += 1) {
      const t = step / 10
      await mouse(
        "mouseMoved",
        { x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t },
        { button: "left", buttons: 1 },
      )
    }
    await frame()
  }
  /** Drags one held mouse gesture through the corners; returns the last screen point. */
  const dragOutline = async (corners) => {
    let previous = await corner(...corners[0])
    await mouse("mouseMoved", previous)
    await press(previous)
    for (const next of corners.slice(1)) {
      const point = await corner(...next)
      await moveHeld(previous, point)
      previous = point
    }
    return previous
  }
  // Corners are projected through the live camera, so wait for a reframe to finish first.
  const settleCamera = async () => {
    let last = ""
    for (let attempt = 0; attempt < 60; attempt += 1) {
      const now = JSON.stringify(await evaluate("window.__zooParkView.camera()"))
      if (now === last) return
      last = now
      await sleep(150)
    }
  }
  const click = async (point) => {
    await mouse("mouseMoved", point)
    await press(point)
    await release(point)
  }

  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await cdp.send("Page.navigate", { url: previewUrl })
  assert((await enterNewPark({ evaluate, sleep })) === "started", "#95: new park did not start")
  await waitFor(
    "the shared 3D park renderer",
    `Boolean(window.__zooParkView?.ready && window.__zooParkView.nodeCount() > 500)`,
  )
  assert(await clickButton("Ⅱ"), "#95: pause control missing")
  await key("Home")
  await settleCamera()

  // A path along y = 7 so habitats east of the starter path touch a walkway.
  assert(await clickButton("Path"), "#95: path tool missing")
  const pathFrom = await tile(5, 7)
  const pathTo = await tile(15, 7)
  await mouse("mouseMoved", pathFrom)
  await press(pathFrom)
  await moveHeld(pathFrom, pathTo)
  await release(pathTo)
  assert((await message()).includes("Path built"), "#95: path stroke failed", await message())

  // --- Desktop: drag an L-shaped outline ------------------------------------------------------
  assert(await clickButton("Draw habitat"), "#95: habitat tool missing")
  assert(await clickButton("⌐ Outline"), "#95: outline shape toggle missing")
  const L = [
    [3, 8],
    [7, 8],
    [7, 10],
    [5, 10],
    [5, 12],
    [3, 12],
    [3, 8],
  ]
  const end = await dragOutline(L)
  await waitFor(
    "the closed L preview",
    `document.querySelector('.outline-status')?.textContent.includes('12 tiles · fence 16') ?? false`,
  )
  const quoted = await statusCents()
  const before = await cashCents()
  await screenshot("test-results/habitat-shapes-l-preview.png")
  await release(end)
  assert(
    (await message()).includes("Habitat #1 fenced · 12 tiles · 16 fence segments"),
    "#95: the L outline did not build",
    await message(),
  )
  assert(before - (await cashCents()) === quoted, "#95: the L cost differs from its preview", {
    before,
    after: await cashCents(),
    quoted,
  })

  // Inspect it through its leg, which lies outside the rectangle of its top part.
  assert(await clickButton("Inspect"), "#95: inspect tool missing")
  await click(await tile(4, 11))
  await waitFor(
    "the L habitat inspector",
    `document.querySelector('.window-title span')?.textContent === 'Habitat #1'`,
  )

  // Move the gate onto the leg's west fence (tap the tile outside it, then confirm).
  assert(await clickButton("Move gate"), "#95: move gate missing")
  await click(await tile(2, 11))
  await waitFor(
    "the leg gate preview",
    `document.querySelector('.gate-controls .guest-thought')?.textContent.includes('Move the service gate here') ?? false`,
  )
  assert(await clickButton("Place gate here"), "#95: gate confirm missing")
  assert((await message()).includes("service gate moved"), "#95: gate move failed", await message())

  // Expand: reshape to the full 4×4, keeping the gate on the west fence.
  const expandAt = await cashCents()
  assert(await clickButton("Reshape fence"), "#95: reshape control missing")
  const expandEnd = await dragOutline([
    [3, 8],
    [7, 8],
    [7, 12],
    [3, 12],
    [3, 8],
  ])
  await waitFor(
    "the expansion preview",
    `document.querySelector('.outline-status')?.textContent.includes('16 tiles') ?? false`,
  )
  const expansion = await statusCents()
  await screenshot("test-results/habitat-shapes-expand-preview.png")
  await release(expandEnd)
  assert(
    (await message()).includes("Habitat #1 reshaped · 16 tiles"),
    "#95: the expansion did not commit",
    await message(),
  )
  assert(expandAt - (await cashCents()) === expansion, "#95: reshape cost differs from preview")

  // Cancel: Escape during a reshape drag builds and spends nothing.
  await click(await tile(4, 9))
  await waitFor(
    "the habitat inspector again",
    `document.querySelector('.window-title span')?.textContent === 'Habitat #1'`,
  )
  assert(await clickButton("Reshape fence"), "#95: reshape control missing again")
  const cancelAt = await cashCents()
  const held = await dragOutline([
    [3, 8],
    [9, 8],
    [9, 10],
  ])
  await key("Escape")
  await release(held)
  assert((await message()).includes("cleared"), "#95: Escape did not cancel", await message())
  assert((await cashCents()) === cancelAt, "#95: a cancelled reshape spent money")
  assert(await clickButton("Stop reshaping"), "#95: stop reshaping missing")

  // Undo the expansion: the L returns and the reshape is refunded exactly.
  assert(await clickButton("Undo"), "#95: undo missing")
  assert(
    (await message()).startsWith("Undid habitat #1 reshape"),
    "#95: reshape undo failed",
    await message(),
  )
  assert((await cashCents()) === expandAt, "#95: reshape undo did not refund exactly")

  // --- Phone: tap the corners of a second L ---------------------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: 390,
    height: 844,
    deviceScaleFactor: 2,
    mobile: true,
  })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await evaluate(`document.querySelector(".window-title button")?.click()`)
  await evaluate(
    `(() => { const card = document.querySelector('details[aria-label="Park guide"]'); if (card) card.open = false })()`,
  )
  await key("Home")
  await settleCamera()
  const touch = (type, points) =>
    cdp.send("Input.dispatchTouchEvent", {
      type,
      touchPoints: points.map(([x, y], id) => ({ x, y, id })),
    })
  const tap = async (point) => {
    await touch("touchStart", [[point.x, point.y]])
    await touch("touchEnd", [])
    await frame()
  }
  assert(await clickButton("Draw habitat"), "#95: habitat tool missing on phone")
  assert(await clickButton("⌐ Outline"), "#95: outline toggle missing on phone")
  // The dock swaps the size inputs for the outline status; let the map settle into its new frame.
  await sleep(500)
  await settleCamera()
  const phoneL = [
    [9, 8],
    [13, 8],
    [13, 10],
    [11, 10],
    [11, 12],
    [9, 12],
  ]
  for (const [x, y] of phoneL) await tap(await corner(x, y))
  const drawn = await evaluate(`document.querySelector(".outline-status")?.dataset.corners ?? ""`)
  assert(
    drawn === "9,8 13,8 13,10 11,10 11,12 9,12",
    "#95: taps did not land on their corners",
    drawn,
  )
  await waitFor(
    "the open phone outline",
    `document.querySelector('.outline-status')?.textContent.includes('Close the fence') ?? false`,
  )
  await screenshot("test-results/habitat-shapes-phone-open.png")
  const phoneBefore = await cashCents()
  await tap(await corner(9, 8))
  assert(
    (await message()).includes("Habitat #2 fenced · 12 tiles · 16 fence segments"),
    "#95: tapped L did not build",
    await message(),
  )
  assert((await cashCents()) < phoneBefore, "#95: the phone L was not charged")
  await screenshot("test-results/habitat-shapes-phone.png")

  // Inspect the tapped L through its leg and expand it to a 4×4 by tapping a new outline.
  assert(await clickButton("Inspect"), "#95: inspect missing on phone")
  await sleep(500)
  await settleCamera()
  await tap(await tile(10, 11))
  await waitFor(
    "the phone habitat inspector",
    `document.querySelector('.window-title span')?.textContent === 'Habitat #2'`,
  )
  assert(await clickButton("Reshape fence"), "#95: reshape missing on phone")
  await sleep(500)
  await settleCamera()
  const phoneExpandAt = await cashCents()
  for (const [x, y] of [
    [9, 8],
    [13, 8],
    [13, 12],
    [9, 12],
  ])
    await tap(await corner(x, y))
  await waitFor(
    "the phone expansion preview",
    `document.querySelector('.outline-status')?.dataset.corners === '9,8 13,8 13,12 9,12'`,
  )
  await tap(await corner(9, 8))
  assert(
    (await message()).includes("Habitat #2 reshaped · 16 tiles"),
    "#95: tapped expansion did not commit",
    await message(),
  )
  assert((await cashCents()) < phoneExpandAt, "#95: the phone expansion was not charged")
  await screenshot("test-results/habitat-shapes-phone-expanded.png")

  // A partial outline cleared by its button builds nothing.
  await evaluate(`document.querySelector(".window-title button")?.click()`)
  assert(await clickButton("Draw habitat"), "#95: habitat tool missing on phone again")
  await sleep(500)
  await settleCamera()
  await tap(await corner(14, 8))
  await tap(await corner(16, 8))
  const clearAt = await cashCents()
  await evaluate(
    `[...document.querySelectorAll('.outline-status button')].find((b) => b.textContent === 'Clear')?.click()`,
  )
  await frame()
  assert((await message()).includes("cleared"), "#95: Clear did not clear", await message())
  assert((await cashCents()) === clearAt, "#95: a cleared outline spent money")

  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  console.log(
    "#95 habitat shapes dogfood passed: dragged L outline builds at its previewed price, inspected through its leg, gate moved onto the leg, expanded by reshape at its previewed price, Escape-cancelled reshape spends nothing, reshape undo refunds exactly (desktop); tapped L outline builds, is inspected and expanded by tapped reshape, and Clear cancels (phone).",
  )
}
