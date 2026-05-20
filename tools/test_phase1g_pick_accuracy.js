const assert = require("assert");

function rankSpatialPickCandidates(candidates, thresholdPx) {
  const valid = (candidates || [])
    .filter(candidate =>
      candidate
      && candidate.screenDistancePx !== null
      && candidate.screenDistancePx !== undefined
      && Number.isFinite(Number(candidate.screenDistancePx))
      && candidate.screenDistancePx >= 0
    )
    .sort((a, b) => a.screenDistancePx - b.screenDistancePx);
  const topCandidates = valid.slice(0, 5);
  const hit = topCandidates.find(candidate => candidate.screenDistancePx <= thresholdPx) || null;
  return {
    hit,
    topCandidates,
    candidateCount: valid.length
  };
}

function rayIntersectsAabb(rayOrigin, rayDirection, min, max) {
  let tMin = 0;
  let tMax = Number.POSITIVE_INFINITY;
  for (const axis of ["x", "y", "z"]) {
    const origin = rayOrigin[axis];
    const direction = rayDirection[axis];
    const minValue = min[axis];
    const maxValue = max[axis];
    if (![origin, direction, minValue, maxValue].every(Number.isFinite)) return null;
    if (Math.abs(direction) < 1e-12) {
      if (origin < minValue || origin > maxValue) return null;
      continue;
    }
    let t1 = (minValue - origin) / direction;
    let t2 = (maxValue - origin) / direction;
    if (t1 > t2) [t1, t2] = [t2, t1];
    tMin = Math.max(tMin, t1);
    tMax = Math.min(tMax, t2);
    if (tMin > tMax) return null;
  }
  return tMin;
}

function rankSpatialRayHits(hits) {
  const valid = (hits || [])
    .filter(hit =>
      hit
      && hit.rayDistance !== null
      && hit.rayDistance !== undefined
      && Number.isFinite(Number(hit.rayDistance))
      && hit.rayDistance >= 0
    )
    .sort((a, b) => a.rayDistance - b.rayDistance);
  return {
    hit: valid[0] || null,
    hits: valid,
    hitCount: valid.length
  };
}

function pickWithRayThenNearest(rayHits, nearestCandidates, thresholdPx) {
  const ray = rankSpatialRayHits(rayHits);
  if (ray.hit) {
    return { pickSource: "spatial_pick_index_ray", feature: ray.hit };
  }
  const nearest = rankSpatialPickCandidates(nearestCandidates, thresholdPx);
  return {
    pickSource: nearest.hit ? "spatial_pick_index" : "miss",
    feature: nearest.hit
  };
}

function visualStateAfterHoverSource(state, sourceId) {
  return {
    ...state,
    hoverSourceId: sourceId,
    visualSelection: state.selectedPickFeatureId
      ? state.visualSelection
      : `source_qa:${sourceId}`,
    bboxVisualSource: state.selectedPickFeatureId ? state.bboxVisualSource : "source_qa_hover"
  };
}

function visualStateAfterPick(state, feature, pickSource) {
  return {
    ...state,
    selectedPickFeatureId: feature.featureId,
    visualSelection: `pick:${feature.sourceId}:${feature.featureId}`,
    interactionSelection: pickSource,
    bboxVisualSource: pickSource === "spatial_pick_index_ray" ? "pick_fallback_ray" : "pick_fallback_nearest"
  };
}

function visualStateAfterMiss(state) {
  return {
    ...state,
    interactionSelection: "miss"
  };
}

function formatPickLabelText(feature, pickSource) {
  const status = pickSource === "spatial_pick_index_ray" ? "ray" : "nearest";
  return `${feature.featureId} | ${status}/${feature.sourceId}`;
}

function test(name, fn) {
  try {
    fn();
    console.log(`ok - ${name}`);
  } catch (err) {
    console.error(`not ok - ${name}`);
    throw err;
  }
}

test("nearest candidate sorting", () => {
  const ranked = rankSpatialPickCandidates([
    { featureId: 3, screenDistancePx: 32 },
    { featureId: 1, screenDistancePx: 8 },
    { featureId: 2, screenDistancePx: 17 }
  ], 36);

  assert.deepStrictEqual(
    ranked.topCandidates.map(candidate => candidate.featureId),
    [1, 2, 3]
  );
  assert.strictEqual(ranked.hit.featureId, 1);
});

test("threshold miss", () => {
  const ranked = rankSpatialPickCandidates([
    { featureId: 1, screenDistancePx: 21 }
  ], 20);

  assert.strictEqual(ranked.hit, null);
  assert.strictEqual(ranked.candidateCount, 1);
});

test("threshold hit", () => {
  const ranked = rankSpatialPickCandidates([
    { featureId: 1, screenDistancePx: 20 }
  ], 20);

  assert.strictEqual(ranked.hit.featureId, 1);
});

test("invalid center skipped", () => {
  const ranked = rankSpatialPickCandidates([
    { featureId: 1, screenDistancePx: NaN },
    { featureId: 2, screenDistancePx: Infinity },
    { featureId: 3, screenDistancePx: -1 },
    { featureId: 4, screenDistancePx: 12 }
  ], 20);

  assert.strictEqual(ranked.candidateCount, 1);
  assert.strictEqual(ranked.hit.featureId, 4);
});

test("ray intersects bbox", () => {
  const distance = rayIntersectsAabb(
    { x: 0, y: 0, z: 0 },
    { x: 1, y: 0, z: 0 },
    { x: 5, y: -1, z: -1 },
    { x: 6, y: 1, z: 1 }
  );

  assert.strictEqual(distance, 5);
});

test("ray misses bbox", () => {
  const distance = rayIntersectsAabb(
    { x: 0, y: 0, z: 0 },
    { x: 1, y: 0, z: 0 },
    { x: 5, y: 2, z: -1 },
    { x: 6, y: 3, z: 1 }
  );

  assert.strictEqual(distance, null);
});

test("nearest ray hit wins", () => {
  const ranked = rankSpatialRayHits([
    { featureId: 10, rayDistance: 120 },
    { featureId: 11, rayDistance: 12 },
    { featureId: 12, rayDistance: 40 }
  ]);

  assert.strictEqual(ranked.hit.featureId, 11);
  assert.deepStrictEqual(ranked.hits.map(hit => hit.featureId), [11, 12, 10]);
});

test("ray miss falls back to nearest center", () => {
  const picked = pickWithRayThenNearest(
    [],
    [
      { featureId: 4, screenDistancePx: 24 },
      { featureId: 5, screenDistancePx: 44 }
    ],
    36
  );

  assert.strictEqual(picked.pickSource, "spatial_pick_index");
  assert.strictEqual(picked.feature.featureId, 4);
});

test("invalid bbox skipped", () => {
  const distance = rayIntersectsAabb(
    { x: 0, y: 0, z: 0 },
    { x: 1, y: 0, z: 0 },
    { x: 5, y: Number.NaN, z: -1 },
    { x: 6, y: 1, z: 1 }
  );
  const ranked = rankSpatialRayHits([
    { featureId: 1, rayDistance: distance },
    { featureId: 2, rayDistance: 8 }
  ]);

  assert.strictEqual(distance, null);
  assert.strictEqual(ranked.hit.featureId, 2);
});

test("hover source sets highlight state", () => {
  const next = visualStateAfterHoverSource({
    selectedPickFeatureId: null,
    visualSelection: "none",
    bboxVisualSource: "none"
  }, "dwg-12d5f1b6");

  assert.strictEqual(next.hoverSourceId, "dwg-12d5f1b6");
  assert.strictEqual(next.visualSelection, "source_qa:dwg-12d5f1b6");
  assert.strictEqual(next.bboxVisualSource, "source_qa_hover");
});

test("selected pick overrides hover style", () => {
  const picked = visualStateAfterPick({
    selectedPickFeatureId: null,
    visualSelection: "source_qa:dwg-12d5f1b6",
    bboxVisualSource: "source_qa_hover"
  }, {
    sourceId: "dwg-12d5f1b6",
    featureId: 99
  }, "spatial_pick_index_ray");
  const hovered = visualStateAfterHoverSource(picked, "dwg-850173d8");

  assert.strictEqual(hovered.hoverSourceId, "dwg-850173d8");
  assert.strictEqual(hovered.visualSelection, "pick:dwg-12d5f1b6:99");
  assert.strictEqual(hovered.bboxVisualSource, "pick_fallback_ray");
});

test("miss keeps source QA visual state", () => {
  const next = visualStateAfterMiss({
    visualSelection: "source_qa:dwg-12d5f1b6",
    interactionSelection: "spatial_pick_index",
    bboxVisualSource: "source_qa_hover"
  });

  assert.strictEqual(next.interactionSelection, "miss");
  assert.strictEqual(next.visualSelection, "source_qa:dwg-12d5f1b6");
  assert.strictEqual(next.bboxVisualSource, "source_qa_hover");
});

test("label text generated correctly", () => {
  const label = formatPickLabelText({
    sourceId: "dwg-12d5f1b6",
    featureId: 123
  }, "spatial_pick_index_ray");

  assert.strictEqual(label, "123 | ray/dwg-12d5f1b6");
});
