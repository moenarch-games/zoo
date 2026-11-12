// #112 animated animals browser proof. Called by capture-park-view-proof.mjs in the same
// production preview and headless Chromium. On the populated Riverside zoo every animal is a
// pinned `zoo-animals` character posed by 3d-lab's Rust runtime: zoo-core's walking/resting
// state selects walk/idle clips, the animal stays pickable by its own ID, the habitat is captured
// close up on desktop and on a phone, reduced motion holds still poses, and a missing release
// falls back to the readable figures instead of stopping the park.
import { enterNewPark } from "./park-library-proof.mjs"

export async function runAnimatedAnimalsAcceptance(context) {
  const { cdp, evaluate, assert, sleep, previewUrl, clickButton, mouse, frame, screenshot } =
    context
  const animals = () => evaluate(`window.__zooParkView?.animals?.() ?? null`)
  const waitFor = async (description, expression, attempts = 100) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    throw new Error(`#112 acceptance timed out waiting for ${description}: ${JSON.stringify(await animals())}`)
  }
  const startRiverside = async () => {
    await cdp.send("Page.navigate", { url: previewUrl })
    assert(
      (await enterNewPark({ evaluate, sleep }, { scenario: "living-starter" })) === "started",
      "#112: Riverside zoo did not start",
    )
    await waitFor(
      "the park renderer",
      `Boolean(window.__zooParkView?.ready && window.__zooParkView.nodeCount() > 500)`,
      200,
    )
  }

  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await startRiverside()
  await waitFor("the animal characters", `window.__zooParkView.animals().instances > 0`)
  const loaded = await animals()
  assert(loaded.loaded && loaded.species.length === 6, "#112: not every species character loaded", loaded)
  const ids = Object.keys(loaded.clips).map(Number)
  for (const id of ids) {
    assert(
      await evaluate(`Boolean(window.__zooParkView.animalPoint(${id}))`),
      `#112: animal #${id} has a character but no pick anchor`,
    )
  }

  // While the park runs, zoo-core's walking and resting animals play walk and idle.
  const seen = new Set()
  for (let sample = 0; sample < 80 && !(seen.has("walk") && seen.has("idle")); sample += 1) {
    for (const clip of Object.values((await animals()).clips)) seen.add(clip)
    await sleep(100)
  }
  assert(seen.has("walk") && seen.has("idle"), "#112: walking/resting animals did not select walk/idle", [...seen])

  // Close up in the habitat: pick animal #1 by its own ID and move the camera to it.
  assert(await clickButton("Ⅱ"), "#112: pause control missing")
  await sleep(300)
  assert(await clickButton("Inspect"), "#112: inspect tool missing")
  const point = await evaluate(`window.__zooParkView.animalPoint(1)`)
  assert(point?.visible, "#112: animal #1 is not on screen", point)
  await mouse("mouseMoved", point)
  await mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
  await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
  await frame()
  await waitFor("animal #1's inspector", `document.querySelector('.animal-card')?.dataset.animalId === '1'`)
  assert(await clickButton("Locate"), "#112: locate control missing")
  await sleep(200)
  // Wheel in on the animal to habitat-watching distance.
  for (let step = 0; step < 10; step += 1) {
    const target = await evaluate(`window.__zooParkView.animalPoint(1)`)
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: target.x, y: target.y, deltaX: 0, deltaY: -400 })
    await sleep(120)
  }
  // Resume so the close-up shows the clips in motion, then pause for a stable capture.
  assert(await clickButton("1×"), "#112: play control missing")
  await sleep(1500)
  assert(await clickButton("Ⅱ"), "#112: pause control missing")
  await sleep(300)
  const close = await animals()
  await screenshot("test-results/animated-animals-desktop.png")
  assert(close.lod.near > 0, "#112: no animal is posed at full detail close up", close.lod)

  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  // Close the inspector and fold the goals so the habitat fills the phone.
  await evaluate(`document.querySelector('.window-title button')?.click()`)
  await evaluate(`document.querySelectorAll('details[open] > summary').forEach((summary) => summary.click())`)
  await sleep(800)
  await screenshot("test-results/animated-animals-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })

  // Reduced motion: every animal holds its clip's still pose.
  await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "reduce" }] })
  assert(await clickButton("1×"), "#112: play control missing")
  await waitFor(
    "still poses under reduced motion",
    `(() => { const a = window.__zooParkView.animals(); return a.lod.still === a.instances })()`,
  )
  await cdp.send("Emulation.setEmulatedMedia", { features: [] })

  // A missing release keeps the park playable with the readable fallback figures.
  const manifestUrl = new URL("zoo-animals/animals.json", previewUrl).href
  cdp.onEvent((event) => {
    if (event.method === "Fetch.requestPaused") {
      cdp.send("Fetch.fulfillRequest", { requestId: event.params.requestId, responseCode: 404, body: "" })
    }
  })
  await cdp.send("Fetch.enable", { patterns: [{ urlPattern: manifestUrl, requestStage: "Request" }] })
  await startRiverside()
  await waitFor("the fallback figures", `window.__zooParkView.animals().loaded === false`)
  const fallback = await animals()
  assert(/HTTP 404/.test(fallback.error ?? ""), "#112: fallback does not name the missing release", fallback)
  assert(
    await evaluate(`window.__zooParkView.animalPoint(1)?.visible === true`),
    "#112: fallback animals are not pickable",
  )
  await cdp.send("Fetch.disable")
  cdp.onEvent(() => {})

  console.log(
    `Animated animals acceptance passed: ${ids.length} Riverside animals as zoo-animals characters (${loaded.species.length} species loaded), zoo-core walking/resting → walk/idle, picked and located animal #1 close up on desktop and phone, still poses under reduced motion, and the readable fallback when the release is missing.`,
  )
}
