// Extension of the existing CDP dogfood; no browser or simulation replacement.
import {verifyLivingWasmReplay} from './living-wasm-replay.mjs'
const sameId = (a, b) => String(a) === String(b)
const distance = (a, b) => Math.hypot(...a.map((n, i) => n - b[i]))
const guestState = (state) => ({arriving:'Arriving', walking_to_habitat:'WalkingToHabitat', viewing:'Viewing', walking_to_exit:'WalkingToExit'}[state] ?? state)
const activityState = (state) => ({walking:'Walking', feeding:'Feeding', resting:'Resting'}[state] ?? state)

// Only progress/completion of the SAME first objective may change on a real watch
// intent. Camera-only operations still compare the complete authoritative snapshot.
function watchPreservesRules(before, after, objectiveId, assert) {
  const normalized = structuredClone(after)
  const original = before.objectives?.find(o => sameId(o.id, objectiveId))
  const observed = normalized.objectives?.find(o => sameId(o.id, objectiveId))
  assert(original && observed, 'Actual core objective missing from watch snapshots')
  assert(observed.progress >= original.progress, 'Watch intent regressed objective')
  assert(!original.completed || observed.completed, 'Watch intent reverted completed objective')
  observed.progress = original.progress
  if (Object.hasOwn(original, 'completed') || Object.hasOwn(observed, 'completed')) observed.completed = original.completed
  assert(JSON.stringify(normalized) === JSON.stringify(before), 'Watch changed rules outside same-objective progress/completion')
}

function actualAuthority(state, assert) {
  assert(state.simulation?.animals && state.simulation?.guests, 'Full actual WASM snapshot required')
  assert(state.simulation.scenario && ['id','kind','seed'].every(key=>state.simulation.scenario[key] === state.scenario[key]),
    'Scenario identity/seed must come from actual immutable core state')
  assert(state.tick === (state.simulation.day-1)*1440+state.simulation.minute_of_day,
    'Activity/follow synchronization clock is not actual authoritative game time')
  assert(state.renderer?.instances?.length > 0 && state.renderer.revision, 'Actual submitted pinned shared-renderer instances required')
  const objective = state.simulation.objectives?.find(o => sameId(o.id, state.objective.id))
  assert(objective && JSON.stringify(objective) === JSON.stringify(state.objective), 'Objective metadata differs from actual core objective')
  for (const animal of state.animals) {
    const core = state.simulation.animals.find(a => sameId(a.id, animal.id))
    assert(core && core.name === animal.name && activityState(core.activity) === animal.activity, 'Animal label/activity fabricated outside core', animal)
    assert(core.activity_target, 'Purposeful activity target absent from actual core', core)
    const node = state.renderer.instances.find(n => n.nodeId === animal.renderNodeId)
    assert(node && node.entityKind === 'animal' && sameId(node.entityId, core.id) && node.visible && node.assetRef === animal.assetRef,
      'Animal must correspond to actually submitted visible renderer instance', animal)
    assert(JSON.stringify(node.position) === JSON.stringify(animal.position), 'Follow observation is not actual submitted world position')
    const habitat = state.simulation.habitats.find(h => sameId(h.id, core.habitat_id))
    assert(habitat && animal.position[0] >= habitat.x && animal.position[0] <= habitat.x + habitat.width &&
      animal.position[2] >= habitat.y && animal.position[2] <= habitat.y + habitat.height, 'Animal left authoritative owned habitat')
    const asset = state.assets.find(a => a.id === node.assetRef)
    assert(asset && asset.clipNames.includes(node.clipName), 'Active shared-renderer clip absent from verified resource', node)
  }
  for (const guest of state.guests) {
    const core = state.simulation.guests.find(g => sameId(g.id, guest.id))
    assert(core && guestState(core.state) === guest.state, 'Guest state fabricated outside core')
    const node = state.renderer.instances.find(n => n.nodeId === guest.renderNodeId)
    assert(node && node.entityKind === 'guest' && sameId(node.entityId, core.id) && node.visible, 'Actual visible guest instance missing')
    const tile = state.simulation.tiles.find(t => t.x === core.x && t.y === core.y)
    assert(tile && ['path','entrance'].includes(tile.kind), 'Guest is not traveling/viewing on an actual valid path')
    if (guest.state === 'Viewing') {
      const habitat = state.simulation.habitats.find(h => sameId(h.id, guest.habitatId))
      const spot = habitat?.viewing_spots.find(s => s.x === core.x && s.y === core.y)
      assert(sameId(core.target_habitat, guest.habitatId) && spot && spot.visible_animals > 0 &&
        spot.capacity > 0 && spot.occupancy <= spot.capacity && guest.viewingPoint.x === core.x && guest.viewingPoint.y === core.y,
        'Guest viewing violates real frontage/visibility/capacity', guest)
      assert(guest.reaction === core.thought && guest.reaction && node.lookTarget, 'Visible guest response must use actual viewing thought/target')
      assert(node.lookTarget[0] >= habitat.x && node.lookTarget[0] <= habitat.x + habitat.width &&
        node.lookTarget[2] >= habitat.y && node.lookTarget[2] <= habitat.y + habitat.height,
        'Actually submitted guest look target is not the viewed habitat')
    }
  }
}

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
  const bytes = async (url) => evaluate(`(async () => {
    const response = await fetch(${JSON.stringify(url)});
    if (!response.ok) throw new Error('Asset evidence fetch failed: '+response.status);
    const digest = await crypto.subtle.digest('SHA-256', await response.arrayBuffer());
    return Array.from(new Uint8Array(digest), n=>n.toString(16).padStart(2,'0')).join('');
  })()`)
  const usablePark = async () => {
    const rect = await evaluate(`(() => {const r=document.querySelector('.park-view canvas')?.getBoundingClientRect();return r?{x:r.x,y:r.y,width:r.width,height:r.height}:null})()`)
    assert(rect && rect.width > 0 && rect.height > 0, 'Actual rendered park disappeared')
    const points = [[.2,.3],[.5,.5],[.8,.7]]
    const visible = await evaluate(`(() => {const r=${JSON.stringify(rect)};return ${JSON.stringify(points)}.filter(([x,y])=>document.elementFromPoint(r.x+r.width*x,r.y+r.height*y)?.closest('.park-view')).length})()`)
    assert(visible >= 2, 'Compact UI obscures meaningful playable park')
    return rect
  }
  const pick = async (animal, phone) => {
    const p = await project(animal.position)
    assert(p?.visible, 'Actual rendered individual is not visible for picking')
    if (phone) { await touch('touchStart', [[p.x,p.y]]); await touch('touchEnd', []) }
    else await click(p)
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
    actualAuthority(initial, assert)
    const originalRequests = await evaluate(`performance.getEntriesByType('resource').map(r=>r.name)`)
    const wasRequested = async (url) => originalRequests.includes(await evaluate(`new URL(${JSON.stringify(url)},location.href).href`))
    assert(initial.replay?.scenarioJson && initial.scenario.sourceUrl && initial.scenario.sourceSha256 && initial.replay.coreWasmUrl,
      'Actual immutable scenario source and production core replay identity required')
    assert(await wasRequested(initial.scenario.sourceUrl) && await wasRequested(initial.replay.coreWasmUrl),
      'Actual scenario/core requests must precede test evidence fetches')
    assert(await bytes(initial.scenario.sourceUrl) === initial.scenario.sourceSha256, 'Scenario source bytes differ from identity')
    assert(await evaluate(`(async()=>await (await fetch(${JSON.stringify(initial.scenario.sourceUrl)})).text())()`) === initial.replay.scenarioJson,
      'Replay scenario is not actual registered source')
    assert(await bytes(initial.replay.coreWasmUrl) === initial.replay.coreWasmSha256, 'Replay core is not actual production WASM resource')
    await usablePark()
    for (const asset of initial.assets) {
      assert(await wasRequested(asset.resourceUrl), 'Runtime resource was not requested before test evidence fetches')
      for (const [url, hash] of [[asset.sourceUrl,asset.sourceSha256], [asset.processingReceiptUrl,asset.processingReceiptSha256], [asset.resourceUrl,asset.resourceSha256]]) {
        assert(url && /^[a-f0-9]{64}$/.test(hash), 'Concrete source/receipt/resource lineage missing', asset)
        assert(await bytes(url) === hash, 'Actual asset bytes differ from claimed lineage', {url,hash})
      }
    }
    assert(await evaluate(`Boolean(document.querySelector('[data-objective-id="${initial.objective.id}"]'))`), 'Visible objective missing')
    assert(await evaluate(`document.querySelectorAll('.park-view button, .park-view .tile').length === 0`), 'DOM game objects forbidden')
    await screenshot(`test-results/living-${label}-opening.png`)
    let unchanged = JSON.stringify(initial.simulation)
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
      actualAuthority(selected, assert)
      watchPreservesRules(JSON.parse(unchanged), selected.simulation, initial.objective.id, assert)
      unchanged = JSON.stringify(selected.simulation)
      samples.push({viewport:label,step:'follow',animalId:animal.id,state:selected,camera:await camera()})
      await screenshot(`test-results/living-${label}-animal-${animal.id}.png`)
      await activate('[data-action="release-follow"]', phone)
      await waitFor('release follow', `window.__zooParkView.livingHabitat().followAnimalId === null`)
      assert(JSON.stringify((await read()).simulation) === unchanged, 'Release follow changed paused simulation')
    }
    const before = await camera()
    await key('e')
    assert((await camera()).yawDegrees !== before.yawDegrees, 'Normal orbit unavailable after release')
    assert(JSON.stringify((await read()).simulation) === unchanged, 'Orbit changed paused simulation')
    await activate('[data-action="resume"]', phone)
    await waitFor('real walking individual', `window.__zooParkView.livingHabitat().animals.some(a=>a.activity === 'Walking')`, 200)
    const live = await read()
    const moving = live.animals.find(a=>a.activity === 'Walking')
    await pick(moving, phone)
    await waitFor('unpaused individual follow', `window.__zooParkView.livingHabitat().followAnimalId === ${JSON.stringify(moving.id)}`)
    let followed = await read()
    let cam = await camera()
    const startPosition = followed.animals.find(a=>sameId(a.id,moving.id)).position
    const startTarget = cam.target
    const motion = [{position:startPosition,target:startTarget}]
    for (let i=0;i<8;i+=1) {
      await waitFor('actual follow tick', `window.__zooParkView.livingHabitat().tick > ${followed.tick}`, 200)
      followed = await read()
      cam = await camera()
      actualAuthority(followed, assert)
      assert(sameId(followed.followAnimalId,moving.id) && sameId(followed.selectedAnimalId,moving.id), 'Follow switched individual across authoritative ticks')
      samples.push({viewport:label,step:'moving-follow',state:followed,camera:cam})
      motion.push({position:followed.animals.find(a=>sameId(a.id,moving.id)).position,target:cam.target})
      await screenshot(`test-results/living-${label}-moving-follow-${i}.png`)
    }
    assert(motion.some(row=>distance(startPosition,row.position) > 1e-5) && motion.some(row=>distance(startTarget,row.target) > 1e-5),
      'Follow camera did not track actually moving submitted individual')
    assert(motion.slice(2).some((row,index)=>{
      const previous = motion[index+1]
      const animalDelta = row.position.map((n,i)=>n-previous.position[i])
      const cameraDelta = row.target.map((n,i)=>n-previous.target[i])
      return animalDelta.reduce((sum,n,i)=>sum+n*cameraDelta[i],0) > 0
    }), 'Camera target never followed actual individual motion after initial focus')
    if (phone) {
      const r = await usablePark()
      const p = [r.x+r.width/2,r.y+r.height/2]
      await touch('touchStart', [p]); await touch('touchMove', [[p[0]+60,p[1]+12]]); await touch('touchEnd', [])
    } else await key('e')
    await waitFor('normal manual navigation overrides follow', `window.__zooParkView.livingHabitat().followAnimalId === null`)
    await activate('[data-action="pause"]', phone)
    const beforeToolbar = JSON.stringify((await read()).simulation)
    await activate('[data-build-category]', phone)
    assert(await buttonPoint('[data-build-item]'), 'Construction category lacks visible usable item')
    await usablePark()
    assert(JSON.stringify((await read()).simulation) === beforeToolbar, 'Opening construction UI changed actual simulation')
    await screenshot(`test-results/living-${label}-construction.png`)
    await activate('[data-action="close-build-category"]', phone)
    const objectiveBefore = (await read()).objective
    if (objectiveBefore.progress === initial.objective.progress) {
      await activate('[data-objective-action]', phone)
    }
    const progressed = await read()
    actualAuthority(progressed, assert)
    assert(sameId(progressed.objective.id, initial.objective.id) && progressed.objective.progress > initial.objective.progress,
      'Real visible player action did not advance authoritative first objective')
    samples.push({viewport:label,step:'objective-progress',state:progressed})
    await activate('[data-action="resume"]', phone)
    const activities = new Set(initial.animals.map(a=>a.activity))
    const guestHistory = new Map()
    const reacted = new Set()
    await waitFor('authoritative activity progression', `window.__zooParkView.livingHabitat().tick > ${initial.tick}`, 200)
    for (let i=0;i<20;i+=1) {
      const state = await read()
      actualAuthority(state, assert)
      for (const animal of state.animals) {
        const original = initial.animals.find(a=>sameId(a.id,animal.id))
        if (original) assert(original.name === animal.name, 'Stable individual identity/name changed during animation')
      }
      for (const guest of state.guests) {
        const phases = guestHistory.get(guest.id) ?? new Set()
        phases.add(guest.state); guestHistory.set(guest.id,phases)
        if (phases.has('WalkingToHabitat') && guest.state === 'Viewing' && guest.reaction) reacted.add(guest.id)
      }
      state.animals.forEach(a=>activities.add(a.activity))
      samples.push({viewport:label,step:'activity',state})
      await screenshot(`test-results/living-${label}-activity-${i}.png`)
      if (activities.has('Walking') && activities.has('Feeding') && activities.has('Resting') && reacted.size > 0) break
      await waitFor('next authoritative tick', `window.__zooParkView.livingHabitat().tick > ${state.tick}`, 200)
    }
    assert(['Walking','Feeding','Resting'].every(a=>activities.has(a)), 'Fixture must demonstrate walking, feeding and resting', [...activities])
    assert(reacted.size > 0, 'No same actual guest approached, viewed and visibly responded')
    // Deliberate network fault on a REAL used resource, never a fake successful asset.
    const unavailable = initial.assets.find(a=>a.kind === 'animal')
    assert(unavailable, 'Required animal resource missing from lineage')
    await cdp.send('Fetch.enable', {patterns:[{urlPattern:new URL(unavailable.resourceUrl,process.env.PREVIEW_URL ?? 'http://127.0.0.1:4173/').href,requestStage:'Request'}]})
    let observedMissingRequest = false
    cdp.onEvent((event) => {
      if (event.method !== 'Fetch.requestPaused') return
      observedMissingRequest = true
      void cdp.send('Fetch.failRequest',{requestId:event.params.requestId,errorReason:'FileNotFound'})
    })
    try {
      await cdp.send('Network.clearBrowserCache')
      await cdp.send('Page.reload',{ignoreCache:true})
      await waitFor('scenario selector or required-asset error', `document.querySelector('[data-scenario-kind="living"]') || document.querySelector('[data-asset-error]')`, 200)
      if (await buttonPoint('[data-scenario-kind="living"]')) await activate('[data-scenario-kind="living"]', phone)
      await waitFor('real required-asset failure', `document.querySelector('[data-asset-error]')?.textContent.includes(${JSON.stringify(unavailable.id)})`, 200)
      assert(observedMissingRequest, 'Asset-failure UI was not caused by actual used resource request')
      assert(!await evaluate(`window.__zooParkView?.livingHabitat?.().renderer?.instances?.some(n=>n.entityKind==='animal' && (n.placeholder || n.assetRef === ${JSON.stringify(unavailable.id)}))`), 'Missing expressive asset silently replaced or claimed loaded')
      await screenshot(`test-results/living-${label}-asset-failure.png`)
      samples.push({viewport:label,step:'asset-failure',assetId:unavailable.id,resourceUrl:unavailable.resourceUrl})
    } finally {
      cdp.onEvent(() => {})
      await cdp.send('Fetch.disable')
    }
  }
  await verifyLivingWasmReplay(samples,assert)
  return samples
}
