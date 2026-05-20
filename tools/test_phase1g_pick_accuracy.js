const assert = require("assert");

function rankSpatialPickCandidates(candidates, thresholdPx) {
  const valid = (candidates || [])
    .filter(candidate =>
      candidate
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
