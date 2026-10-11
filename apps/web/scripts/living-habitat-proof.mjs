// Extension of the existing CDP dogfood; no browser or simulation replacement.
export async function captureLivingHabitatProof({cdp, evaluate, waitFor, assert, camera, project, click, touch, key, screenshot, samples}) {
  const read = () => evaluate(`window.__zooParkView.livingHabitat()`)
  const buttonPoint = async (selector) => evaluate(`(() => {
    const node = document.querySelector(${JSON.stringify(selector)})
    if (!node || node.disabled) return null
    const r = node.getBoundingClientRect()
    if (r.width <= 0 || r.height <= 0 || r.left < 0 || r.top < 0 || r.right > innerWidth || r.bottom > innerHeight) return null
    return {x:r.left+r.width/2,y:r.top+r.height/2}
  })()`)
  const activate = async (selector, phone) => {
    const p = await buttonPoint(selector)
    assert(p, `Visible usable control missing: ${selector}`)
    if (phone) {
      await touch('touchStart', [[p.x,p.y]])
      await touch('touchEnd', [])
    } else await click(p)
  }
  for (const phone of [false, true]) {
    const label = phone ? 'phone' : 'desktop'
    await cdp.send('Emulation.setDeviceMetricsOverride', {width:phone?390:1440,height:phone?844:900,deviceScaleFactor:phone?2:1,mobile:phone})
    await cdp.send('Emulation.setTouchEmulationEnabled', {enabled:phone,maxTouchPoints:5})
    await cdp.send('Page.navigate', {url:process.env.PREVIEW_URL ?? 'http://127.0.0.1:4173/'})
    await waitFor('real renderer', `window.__zooParkView?.ready`)
    await screenshot(`test-results/living-${label}-baseline.png`)
    await waitFor('read-only authoritative living-habitat observation', `typeof window.__zooParkView?.livingHabitat === 'function'`)
    await activate('[data-scenario-kind="living"]', phone)
    await waitFor('living scenario', `window.__zooParkView.livingHabitat().scenario?.kind === 'living'`)
    await activate('[data-action="pause"]', phone)
    const initial = await read()
    samples.push({viewport:label,step:'opening',state:initial,camera:await camera()})
    assert(initial.scenario.id && initial.scenario.seed !== undefined, 'Scenario identity/seed missing', initial.scenario)
    assert(initial.objective?.id && initial.objective.target && initial.objective.progress !== undefined, 'Authoritative first objective missing', initial.objective)
    assert(initial.cameraObjectiveId === initial.objective.id, 'Opening camera must frame first objective')
    assert(initial.animals.length >= 2 && new Set(initial.animals.map(a=>a.id)).size === initial.animals.length, 'Distinct stable individual animals required')
    assert(initial.animals.every(a=>a.name && a.activity && a.position), 'Individual name/activity/position missing')
    assert(initial.guests.some(g=>g.state === 'Viewing' && g.viewingPoint && g.habitatId), 'Valid authoritative guest viewing required')
    assert(initial.assets.length > 0 && initial.assets.every(a=>a.loaded && a.provenance && !a.placeholder), 'Missing or placeholder runtime assets', initial.assets)
    assert(await evaluate(`Boolean(document.querySelector('[data-objective-id="${initial.objective.id}"]'))`), 'Visible objective missing')
    assert(await evaluate(`document.querySelectorAll('.park-view button, .park-view .tile').length === 0`), 'DOM game objects forbidden')
    await screenshot(`test-results/living-${label}-opening.png`)
    const unchanged = JSON.stringify(initial.simulation)
    for (const animal of initial.animals.slice(0,2)) {
      const p = await project(animal.position)
      assert(p.x >= 0 && p.x < (phone?390:1440) && p.y >= 0 && p.y < (phone?844:900), 'Animal pick outside viewport', p)
      if (phone) {
        await touch('touchStart', [[p.x,p.y]])
        await touch('touchEnd', [])
      } else await click(p)
      await waitFor('individual follow', `window.__zooParkView.livingHabitat().followAnimalId === ${JSON.stringify(animal.id)}`)
      const selected = await read()
      assert(selected.selectedAnimalId === animal.id, 'Picked habitat instead of individual')
      assert(await evaluate(`(() => {const n=document.querySelector('[data-animal-watch]');return n?.textContent.includes(${JSON.stringify(animal.name)}) && n?.textContent.includes(${JSON.stringify(animal.activity)})})()`), 'Watch label must show authoritative individual name/activity')
      assert(!await evaluate(`Boolean(document.querySelector('.window-title'))`), 'Watching opened management window')
      assert(JSON.stringify(selected.simulation) === unchanged, 'Camera/selection changed paused simulation')
      samples.push({viewport:label,step:'follow',animalId:animal.id,state:selected,camera:await camera()})
      await screenshot(`test-results/living-${label}-animal-${animal.id}.png`)
      await activate('[data-action="release-follow"]', phone)
      await waitFor('release follow', `window.__zooParkView.livingHabitat().followAnimalId === null`)
    }
    const before = await camera()
    await key('e')
    assert((await camera()).yawDegrees !== before.yawDegrees, 'Normal orbit unavailable after release')
    assert(JSON.stringify((await read()).simulation) === unchanged, 'Orbit changed paused simulation')
    await activate('[data-action="resume"]', phone)
    const activities = new Set(initial.animals.map(a=>a.activity))
    await waitFor('authoritative activity progression', `window.__zooParkView.livingHabitat().tick > ${initial.tick}`, 200)
    for (let i=0;i<20;i+=1) {
      const state = await read()
      state.animals.forEach(a=>activities.add(a.activity))
      samples.push({viewport:label,step:'activity',state})
      await screenshot(`test-results/living-${label}-activity-${i}.png`)
      if (activities.has('Walking') && activities.has('Feeding') && activities.has('Resting')) break
      await waitFor('next authoritative tick', `window.__zooParkView.livingHabitat().tick > ${state.tick}`, 200)
    }
    assert(['Walking','Feeding','Resting'].every(a=>activities.has(a)), 'Fixture must demonstrate walking, feeding and resting', [...activities])
  }
  return samples
}
