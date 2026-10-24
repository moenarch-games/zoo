// #59 S1 browser acceptance: bounded renderer lifetime and clear contract failures.
//
// Called by capture-park-view-proof.mjs after the existing journeys, against the same production
// preview and real headless Chromium. The park view is unmounted and remounted through the
// declared `?zoo-fault-injection=1` hook (`window.__zooFaults.remountParkView`), so every cycle
// runs the real shared-renderer load/dispose path. Resource counts come from the shared
// renderer's own work observations and from Three.js' `renderer.info`, read through
// `window.__zooParkView.resources()`. The incompatible contract is a real catalog document with
// an unsupported schema version, served at the production catalog HTTP boundary.

import { enterNewPark } from "./park-library-proof.mjs"

const REMOUNTS = 12
const PANEL = `[aria-label="Park recovery"]`

export async function runRendererLifetimeAcceptance(context) {
  const { cdp, evaluate, assert, sleep, previewUrl, screenshot } = context
  const warnings = []
  let catalogBody = null
  cdp.onEvent((event) => {
    if (
      event.method === "Runtime.consoleAPICalled" &&
      (event.params.type === "warning" || event.params.type === "error")
    ) {
      warnings.push(event.params.args?.map((arg) => arg.value ?? arg.description).join(" ") ?? "")
    } else if (event.method === "Log.entryAdded") {
      warnings.push(event.params.entry.text)
    } else if (event.method === "Fetch.requestPaused" && catalogBody !== null) {
      cdp.send("Fetch.fulfillRequest", {
        requestId: event.params.requestId,
        responseCode: 200,
        responseHeaders: [{ name: "Content-Type", value: "application/json" }],
        body: catalogBody,
      })
    }
  })
  await cdp.send("Log.enable")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })

  const waitFor = async (description, expression, attempts = 100) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({
      renderer: document.querySelector('.park-view')?.dataset.renderer ?? null,
      lifetime: window.__zooRendererLifetime ?? null,
      resources: window.__zooParkView?.resources?.() ?? null,
      panel: document.querySelector(${JSON.stringify(PANEL)})?.textContent ?? null,
    })`)
    throw new Error(
      `#59 lifetime acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`,
    )
  }
  const navigate = async (query) => {
    const url = new URL(previewUrl)
    url.search = query
    await cdp.send("Page.navigate", { url: url.href })
    await enterNewPark({ evaluate, sleep })
  }
  const resources = () => evaluate(`window.__zooParkView.resources()`)
  const lifetime = () => evaluate(`({...window.__zooRendererLifetime})`)
  const mounted = (mounts) => `Boolean(
    window.__zooRendererLifetime?.mounts === ${mounts} &&
    window.__zooParkView?.ready && window.__zooParkView.nodeCount() > 500 &&
    window.__zooParkView.resources() !== null &&
    document.querySelector('.park-view[data-renderer="ready"] canvas.park-canvas'))`

  // --- 1. Repeated load/dispose returns to the same live resources ------------------------------
  await navigate("?zoo-fault-injection=1")
  await waitFor("the first park-view mount", mounted(1))
  assert(await context.clickButton("Ⅱ"), "#59: pause control missing")
  await sleep(500)
  const baseline = await resources()
  assert(
    baseline.gpuGeometries > 0 && baseline.objects > 0,
    "#59: baseline resources not observed",
    baseline,
  )
  const rows = [{ mount: 1, ...baseline }]
  // Three.js 0.186's WebGLRenderer.dispose() leaves one internal texture counted that no scene
  // object owns. It must stay constant per release (never growing with scene content) and is
  // reclaimed with the context, which every release force-loses.
  let releaseResidue = null
  for (let cycle = 2; cycle <= REMOUNTS + 1; cycle += 1) {
    await evaluate(`window.__zooFaults.remountParkView()`)
    await waitFor(`park-view mount ${cycle}`, mounted(cycle))
    await sleep(150)
    const state = await lifetime()
    assert(
      state.releases === cycle - 1,
      `#59: mount ${cycle} did not release exactly the previous renderer`,
      state,
    )
    assert(
      state.lastRelease?.gpuGeometries === 0 && state.lastRelease.gpuTextures <= 1,
      `#59: disposing mount ${cycle - 1} left scene GPU resources behind`,
      state.lastRelease,
    )
    releaseResidue ??= state.lastRelease.gpuTextures
    assert(
      state.lastRelease.gpuTextures === releaseResidue,
      `#59: release residue changed at mount ${cycle - 1}`,
      { releaseResidue, lastRelease: state.lastRelease },
    )
    assert(
      (await evaluate(`document.querySelectorAll('canvas.park-canvas').length`)) === 1,
      `#59: mount ${cycle} left more than one park canvas`,
    )
    rows.push({ mount: cycle, ...(await resources()) })
  }
  const final = rows.at(-1)
  for (const field of [
    "objects",
    "geometries",
    "materials",
    "gpuGeometries",
    "gpuTextures",
    "gpuPrograms",
  ]) {
    assert(final[field] === baseline[field], `#59: ${field} drifted across ${REMOUNTS} remounts`, {
      baseline,
      final,
    })
  }
  const contextWarnings = warnings.filter((text) => /WebGL contexts|CONTEXT_LOST/i.test(text))
  assert(
    contextWarnings.length === 0,
    "#59: remounts exhausted or leaked WebGL contexts",
    contextWarnings.slice(0, 5),
  )
  assert(
    await evaluate(`window.__zooParkView.parkFootprintInFrame()`),
    "#59: remounted view does not frame the park",
  )
  console.table(rows)

  // --- 2. An incompatible content contract stops at a readable failure screen --------------------
  const catalogUrl = new URL("content/catalog.json", previewUrl).href
  const definition = await (await fetch(catalogUrl)).json()
  definition.schema_version = 2
  catalogBody = Buffer.from(JSON.stringify(definition)).toString("base64")
  await cdp.send("Fetch.enable", {
    patterns: [{ urlPattern: catalogUrl, requestStage: "Request" }],
  })
  await navigate("")
  await waitFor(
    "the startup failure screen",
    `Boolean(document.querySelector(${JSON.stringify(PANEL)})?.textContent.includes('Park could not start'))`,
  )
  const panel = await evaluate(`document.querySelector(${JSON.stringify(PANEL)}).textContent`)
  assert(
    panel.includes("schema_version") && panel.includes("simulation contract"),
    "#59: failure screen does not name the incompatible contract",
    panel,
  )
  assert(
    !(await evaluate(`Boolean(window.__zooParkView?.ready)`)),
    "#59: incompatible catalog booted a park",
  )
  const exported = JSON.parse(await evaluate(`window.__zooDiagnostics.exportText()`))
  assert(
    exported.browser_failures.some(
      (failure) =>
        failure.source === "asset" &&
        failure.recovery === "frozen" &&
        failure.message.includes("schema_version"),
    ),
    "#59: contract failure missing from diagnostics",
    exported.browser_failures,
  )
  for (const label of ["Download diagnostics", "Copy diagnostics"]) {
    assert(
      await evaluate(
        `[...document.querySelectorAll(${JSON.stringify(`${PANEL} button`)})].some((button) => button.textContent === ${JSON.stringify(label)})`,
      ),
      `#59: failure screen lacks ${label}`,
    )
  }
  await screenshot("test-results/startup-contract-failure.png")
  catalogBody = null
  await cdp.send("Fetch.disable")

  console.log(
    `Renderer lifetime acceptance passed: ${REMOUNTS} remounts released every renderer and GPU resource and returned to baseline; an incompatible catalog contract stops at a readable failure screen with diagnostics.`,
  )
}
