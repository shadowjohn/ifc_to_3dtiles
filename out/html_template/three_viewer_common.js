import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { TilesRenderer } from "3d-tiles-renderer";

const AUTO_RENDER_SCALE_MOVING = 0.5;
const AUTO_RENDER_SCALE_RESTORED = 1.0;
const RESTORE_RENDER_SCALE_DELAY_MS = 260;
const TILESET_OPTIONS = [
  { mode: "flat", label: "平面", url: "./tileset.json" },
  { mode: "smooth90", label: "90", url: "./tileset_smooth_90.json" },
  { mode: "smooth", label: "平滑", url: "./tileset_smooth.json" }
];
const MOVE_DIRECTIONS = {
  n: new THREE.Vector3(0, 1, 0),
  ne: new THREE.Vector3(1, 1, 0).normalize(),
  e: new THREE.Vector3(1, 0, 0),
  se: new THREE.Vector3(1, -1, 0).normalize(),
  s: new THREE.Vector3(0, -1, 0),
  sw: new THREE.Vector3(-1, -1, 0).normalize(),
  w: new THREE.Vector3(-1, 0, 0),
  nw: new THREE.Vector3(-1, 1, 0).normalize(),
  center: new THREE.Vector3(0, 0, 0)
};
const BASEMAPS = {
  emap5: {
    label: "EMAP5",
    tiles: ["https://wmts.nlsc.gov.tw/wmts/EMAP5/default/GoogleMapsCompatible/{z}/{y}/{x}"],
    tileSize: 256,
    attribution: "NLSC EMAP5"
  },
  googleSatellite: {
    label: "Google 航照圖",
    tiles: [
      "https://mt0.google.com/vt/lyrs=s&x={x}&y={y}&z={z}",
      "https://mt1.google.com/vt/lyrs=s&x={x}&y={y}&z={z}",
      "https://mt2.google.com/vt/lyrs=s&x={x}&y={y}&z={z}",
      "https://mt3.google.com/vt/lyrs=s&x={x}&y={y}&z={z}"
    ],
    tileSize: 256,
    attribution: "Google"
  },
  googleRoadmap: {
    label: "Google 街景圖",
    tiles: [
      "https://mt0.google.com/vt/lyrs=m&x={x}&y={y}&z={z}",
      "https://mt1.google.com/vt/lyrs=m&x={x}&y={y}&z={z}",
      "https://mt2.google.com/vt/lyrs=m&x={x}&y={y}&z={z}",
      "https://mt3.google.com/vt/lyrs=m&x={x}&y={y}&z={z}"
    ],
    tileSize: 256,
    attribution: "Google"
  },
  osm: {
    label: "OSM",
    tiles: ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"],
    tileSize: 256,
    attribution: "OpenStreetMap"
  }
};
const DEFAULT_VIEWER_CONFIG = {
  basemap: "emap5",
  normalMode: "flat",
  renderScale: AUTO_RENDER_SCALE_RESTORED,
  autoRenderScale: true,
  visualPreset: "sketchfab",
  focus: false,
  explode: 0,
  moveOut: 0,
  moveDirection: "sw",
  ground: true,
  backgroundColor: "#dddddd",
  toneMapping: "filmic",
  toneMappingExposure: 1.2,
  ambientIntensity: 2.35,
  directionLightIntensity: 1.0,
  fillLightIntensity: 0.35,
  cameraFillLightIntensity: 0.9,
  reflectionBoost: 0.5,
  reflectionRoughness: 0.24,
  contentUpAxis: "auto",
  doubleSided: "auto"
};

const textDecoder = new TextDecoder("utf-8");
const batchTableCache = new Map();
const SELECTION_POSITION_KEY_SCALE = 100000;

function getThreeViewerConfig(mode, query) {
  const config = window.IFC_VIEWER_CONFIG || {};
  const modeKey = mode === "maplibre" ? "maplibreThree" : "three";
  const merged = Object.assign(
    {},
    DEFAULT_VIEWER_CONFIG,
    readViewerConfig(config.common),
    readViewerConfig(config[modeKey])
  );
  return applyQueryOverrides(merged, query);
}

function readViewerConfig(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : {};
}

function applyQueryOverrides(config, query) {
  const result = Object.assign({}, config);
  const mappings = {
    normalMode: ["normalMode", "normal-mode", "normal"],
    basemap: ["basemap", "baseMap", "base-map"],
    renderScale: ["renderScale", "render-scale", "scale"],
    autoRenderScale: ["autoRenderScale", "auto-render-scale", "autoScale", "auto-scale"],
    visualPreset: ["visualPreset", "visual-preset", "preset"],
    focus: ["focus"],
    explode: ["explode"],
    moveOut: ["moveOut", "move-out"],
    moveDirection: ["moveDirection", "move-direction"],
    ground: ["ground"],
    backgroundColor: ["backgroundColor", "background-color", "bg", "bgColor", "bg-color"],
    toneMapping: ["toneMapping", "tone-mapping", "tonemap"],
    toneMappingExposure: ["toneMappingExposure", "tone-mapping-exposure", "exposure"],
    ambientIntensity: ["ambientIntensity", "ambient-intensity", "ambient"],
    directionLightIntensity: ["directionLightIntensity", "direction-light-intensity", "directionalLight", "directional-light"],
    fillLightIntensity: ["fillLightIntensity", "fill-light-intensity", "fillLight", "fill-light"],
    cameraFillLightIntensity: ["cameraFillLightIntensity", "camera-fill-light-intensity", "cameraLightIntensity", "camera-light-intensity"],
    reflectionBoost: ["reflectionBoost", "reflection-boost", "reflect", "reflection"],
    reflectionRoughness: ["reflectionRoughness", "reflection-roughness", "roughness"],
    contentUpAxis: ["contentUpAxis", "content-up-axis", "upAxis", "up-axis"],
    doubleSided: ["doubleSided", "double-sided", "doubleSide", "double-side"]
  };
  Object.keys(mappings).forEach(key => {
    for (const name of mappings[key]) {
      if (query.has(name)) {
        result[key] = query.get(name);
        return;
      }
    }
  });
  return result;
}

function normalizeBasemap(value, fallback) {
  const normalized = String(value || "")
    .trim()
    .toLowerCase()
    .replace(/[\s_-]/g, "");
  const aliases = {
    emap5: "emap5",
    nlsc: "emap5",
    google: "googleRoadmap",
    googleroad: "googleRoadmap",
    googleroadmap: "googleRoadmap",
    roadmap: "googleRoadmap",
    street: "googleRoadmap",
    googlesatellite: "googleSatellite",
    satellite: "googleSatellite",
    googleaerial: "googleSatellite",
    aerial: "googleSatellite",
    osm: "osm",
    openstreetmap: "osm"
  };
  return aliases[normalized] || fallback;
}

function normalizeMoveDirection(value, fallback) {
  const normalized = String(value || "").trim().toLowerCase();
  return MOVE_DIRECTIONS[normalized] ? normalized : fallback;
}

function normalizeContentUpAxis(value) {
  const normalized = String(value || "").trim().toLowerCase();
  if (normalized === "y" || normalized === "yup" || normalized === "y-up") {
    return "y";
  }
  if (normalized === "z" || normalized === "zup" || normalized === "z-up") {
    return "z";
  }
  return "auto";
}

function normalizeAutoBoolean(value) {
  if (String(value).trim().toLowerCase() === "auto") {
    return "auto";
  }
  if (value == null || value === "") {
    return "auto";
  }
  return toBool(value, false);
}

function normalizeVisualPreset(value, fallback) {
  const normalized = String(value || "").trim().toLowerCase().replace(/[\s_-]/g, "");
  if (normalized === "sketchfab" || normalized === "sf") {
    return "sketchfab";
  }
  if (normalized === "default" || normalized === "standard") {
    return "default";
  }
  return fallback;
}

function normalizeCssColor(value, fallback) {
  const text = String(value || "").trim();
  if (/^#[0-9a-f]{6}$/i.test(text) || /^#[0-9a-f]{3}$/i.test(text)) {
    return text;
  }
  return fallback;
}

function normalModeToIndex(value) {
  if (typeof value === "number" && TILESET_OPTIONS[value]) {
    return value;
  }
  const normalized = String(value || "")
    .trim()
    .toLowerCase()
    .replace(/[\s_-]/g, "");
  if (normalized === "1" || normalized === "90" || normalized === "smooth90") {
    return 1;
  }
  if (normalized === "2" || normalized === "smooth" || normalized === "fullsmooth") {
    return 2;
  }
  return 0;
}

function toBool(value, fallback) {
  if (typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    return value !== 0;
  }
  const normalized = String(value || "").trim().toLowerCase();
  if (["1", "true", "yes", "y", "on"].includes(normalized)) {
    return true;
  }
  if (["0", "false", "no", "n", "off"].includes(normalized)) {
    return false;
  }
  return fallback;
}

function clampNumber(value, min, max, fallback) {
  const number = Number(value);
  if (!Number.isFinite(number)) {
    return fallback;
  }
  return Math.min(max, Math.max(min, number));
}

function getToneMappingMode(value) {
  const normalized = String(value || "").trim().toLowerCase().replace(/[\s_-]/g, "");
  if (normalized === "filmic" || normalized === "aces" || normalized === "acesfilmic") {
    return THREE.ACESFilmicToneMapping;
  }
  if (normalized === "cineon") {
    return THREE.CineonToneMapping;
  }
  if (normalized === "reinhard") {
    return THREE.ReinhardToneMapping;
  }
  if (normalized === "none" || normalized === "linear") {
    return THREE.NoToneMapping;
  }
  return THREE.ACESFilmicToneMapping;
}

export async function bootIfcThreeViewer(options = {}) {
  const mode = options.mode || "three";
  const query = new URLSearchParams(location.search);
  const viewerConfig = getThreeViewerConfig(mode, query);
  const initialRenderScale = clampNumber(viewerConfig.renderScale, 0.5, 1.0, AUTO_RENDER_SCALE_RESTORED);
  const initialAutoRenderScale = toBool(viewerConfig.autoRenderScale, true);
  const visualPreset = normalizeVisualPreset(viewerConfig.visualPreset, "sketchfab");
  const presetBackground = visualPreset === "sketchfab" ? "#dddddd" : "#bfdde7";
  const root = document.getElementById("viewerRoot");
  const status = document.getElementById("measureResult");
  const state = {
    mode,
    root,
    status,
    viewerConfig,
    visualPreset,
    backgroundColor: normalizeCssColor(viewerConfig.backgroundColor, presetBackground),
    toneMapping: viewerConfig.toneMapping || (visualPreset === "sketchfab" ? "filmic" : "filmic"),
    toneMappingExposure: clampNumber(viewerConfig.toneMappingExposure, 0.4, 2.5, visualPreset === "sketchfab" ? 1.2 : 1.0),
    ambientIntensity: clampNumber(viewerConfig.ambientIntensity, 0, 6, visualPreset === "sketchfab" ? 2.35 : 1.8),
    directionLightIntensity: clampNumber(viewerConfig.directionLightIntensity, 0, 6, visualPreset === "sketchfab" ? 1.0 : 2.8),
    fillLightIntensity: clampNumber(viewerConfig.fillLightIntensity, 0, 3, visualPreset === "sketchfab" ? 0.35 : 0),
    cameraFillLightIntensity: clampNumber(viewerConfig.cameraFillLightIntensity, 0, 4, visualPreset === "sketchfab" ? 0.9 : 0),
    reflectionBoost: clampNumber(viewerConfig.reflectionBoost, 0, 1, visualPreset === "sketchfab" ? 0.5 : 0),
    reflectionRoughness: clampNumber(viewerConfig.reflectionRoughness, 0.02, 1, visualPreset === "sketchfab" ? 0.24 : 0.65),
    debugScale: query.get("debugScale") === "1",
    scene: null,
    camera: null,
    tilesCamera: null,
    renderer: null,
    controls: null,
    map: null,
    customLayerReady: false,
    localTransform: null,
    tiles: null,
    raycaster: new THREE.Raycaster(),
    pointer: new THREE.Vector2(),
    rootOriginEcef: new THREE.Vector3(),
    rootOriginLngLatAlt: null,
    modelCenter: new THREE.Vector3(),
    groundLayer: null,
    cameraFillLight: null,
    environmentConfigured: false,
    selected: null,
    selectedOverlayGroup: new THREE.Group(),
    selectedBaseMesh: null,
    selectedBaseWasVisible: true,
    selectedKey: "",
    focusEnabled: toBool(viewerConfig.focus, false),
    moveDirectionKey: normalizeMoveDirection(viewerConfig.moveDirection, "sw"),
    moveOut: clampNumber(viewerConfig.moveOut, 0, 180, 0),
    explode: clampNumber(viewerConfig.explode, 0, 160, 0),
    measureMode: "",
    measurePoints: [],
    measureLayer: new THREE.Group(),
    renderScaleTarget: initialRenderScale,
    activeRenderScale: initialAutoRenderScale ? AUTO_RENDER_SCALE_RESTORED : initialRenderScale,
    autoRenderScale: initialAutoRenderScale,
    restoreTimer: null,
    normalModeIndex: normalModeToIndex(viewerConfig.normalMode),
    basemap: normalizeBasemap(viewerConfig.basemap, "emap5"),
    groundVisible: toBool(viewerConfig.ground, true),
    baseMaterialState: new WeakMap(),
    loadedTileBytes: 0,
    loadedTileUrls: new Set(),
    restoreSelectionPending: null,
    contentUpAxis: "z",
    contentUpAxisMode: "auto",
    doubleSided: false,
    doubleSidedMode: "auto",
    isGlbTilesOutput: false,
    glbTileScenes: new Set(),
    frame: {
      lastTime: performance.now(),
      lastPanelTime: 0,
      fps: 0,
      frameMs: 0
    }
  };

  window.ifcThreeViewer = state;
  window.batchTableCache = batchTableCache;

  setupCommonScene(state);
  bindToolbar(state);

  if (mode === "maplibre") {
    await setupMapLibreViewer(state);
  } else {
    setupPureThreeViewer(state);
  }

  await resolveContentOptions(state);
  await loadTileset(state, TILESET_OPTIONS[state.normalModeIndex].url);
  if (mode === "three") {
    animatePureThree(state);
  }
}

function setupCommonScene(state) {
  state.scene = new THREE.Scene();
  const background = new THREE.Color(state.backgroundColor);
  state.scene.background = background;

  if (state.visualPreset === "sketchfab") {
    state.scene.add(new THREE.HemisphereLight(0xffffff, 0x4d3833, state.ambientIntensity));
    const warm = new THREE.DirectionalLight(0xffe0b5, state.directionLightIntensity);
    warm.position.set(-92, 54, 168);
    state.scene.add(warm);
    const cool = new THREE.DirectionalLight(0xc7ccff, state.directionLightIntensity * 0.72);
    cool.position.set(16, 104, -57);
    state.scene.add(cool);
    const blueFill = new THREE.DirectionalLight(0x6cbbff, state.fillLightIntensity);
    blueFill.position.set(106, 104, 74);
    state.scene.add(blueFill);
    if (state.cameraFillLightIntensity > 0) {
      state.cameraFillLight = new THREE.DirectionalLight(0xffffff, state.cameraFillLightIntensity);
      state.cameraFillLight.position.set(0, -1, 1);
      state.scene.add(state.cameraFillLight);
    }
  } else {
    state.scene.add(new THREE.AmbientLight(0xffffff, state.ambientIntensity));
    const sun = new THREE.DirectionalLight(0xffffff, state.directionLightIntensity);
    sun.position.set(200, -260, 420);
    state.scene.add(sun);
  }

  state.selectedOverlayGroup.name = "selected-overlay";
  state.scene.add(state.selectedOverlayGroup);
  state.measureLayer.name = "measure-layer";
  state.scene.add(state.measureLayer);
}

function configureThreeRenderer(state) {
  if (!state.renderer) {
    return;
  }
  state.renderer.outputColorSpace = THREE.SRGBColorSpace;
  state.renderer.toneMapping = getToneMappingMode(state.toneMapping);
  state.renderer.toneMappingExposure = state.toneMappingExposure;
  configureThreeEnvironment(state);
}

function configureThreeEnvironment(state) {
  if (!state.renderer || !state.scene || state.environmentConfigured || state.reflectionBoost <= 0) {
    return;
  }
  const pmremGenerator = new THREE.PMREMGenerator(state.renderer);
  state.scene.environment = pmremGenerator.fromScene(new RoomEnvironment(), 0.04).texture;
  state.environmentConfigured = true;
}

function setupPureThreeViewer(state) {
  const canvas = document.getElementById("threeCanvas");
  state.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: false });
  configureThreeRenderer(state);
  state.renderer.setClearColor(new THREE.Color(state.backgroundColor), 1);
  state.renderer.setPixelRatio(window.devicePixelRatio * state.activeRenderScale);
  state.renderer.setSize(rootWidth(state), rootHeight(state), false);

  state.camera = new THREE.PerspectiveCamera(55, rootWidth(state) / rootHeight(state), 0.1, 200000);
  state.camera.position.set(160, -260, 160);
  state.camera.up.set(0, 0, 1);
  state.tilesCamera = state.camera;

  state.controls = new OrbitControls(state.camera, canvas);
  state.controls.target.set(0, 0, 0);
  state.controls.enableDamping = true;
  state.controls.dampingFactor = 0.12;
  state.controls.screenSpacePanning = true;
  state.controls.mouseButtons = {
    LEFT: THREE.MOUSE.ROTATE,
    MIDDLE: THREE.MOUSE.DOLLY,
    RIGHT: THREE.MOUSE.PAN
  };
  state.controls.touches = {
    ONE: THREE.TOUCH.ROTATE,
    TWO: THREE.TOUCH.DOLLY_PAN
  };
  state.controls.addEventListener("start", () => beginRenderInteraction(state));
  state.controls.addEventListener("end", () => endRenderInteraction(state));

  state.groundLayer = createGroundLayer();
  state.groundLayer.visible = state.groundVisible;
  state.scene.add(state.groundLayer);

  window.addEventListener("resize", () => resizePureThree(state));
  canvas.addEventListener("pointerdown", event => {
    if (event.button === 0 && !state.measureMode) {
      selectAtClientPoint(state, event.clientX, event.clientY);
    }
  });
  canvas.addEventListener("click", event => {
    if (state.measureMode) {
      addMeasurePoint(state, event.clientX, event.clientY);
    }
  });
  canvas.addEventListener("contextmenu", event => {
    event.preventDefault();
    finishMeasure(state);
  });
  canvas.addEventListener("pointerdown", () => beginRenderInteraction(state));
  window.addEventListener("pointerup", () => endRenderInteraction(state));
  ["wheel", "keydown"].forEach(name => {
    window.addEventListener(name, () => markInteraction(state), { passive: true });
  });
}

async function setupMapLibreViewer(state) {
  const map = new maplibregl.Map({
    container: "map",
    style: buildMapLibreStyle(state.basemap),
    center: [121.0, 24.0],
    zoom: 18,
    pitch: 58,
    bearing: 0,
    maxPitch: 78,
    renderWorldCopies: false,
    pixelRatio: window.devicePixelRatio * state.activeRenderScale,
    canvasContextAttributes: { antialias: true, alpha: true }
  });
  state.map = map;
  map.addControl(new maplibregl.NavigationControl({ visualizePitch: true }), "bottom-right");
  map.dragRotate.disable();
  map.touchZoomRotate.disableRotation();
  if (map.scrollZoom && map.scrollZoom.setWheelZoomRate) {
    map.scrollZoom.setWheelZoomRate(1 / 300);
  }
  map.on("movestart", () => markInteraction(state));
  map.on("move", () => markInteraction(state));
  map.on("wheel", () => markInteraction(state));
  map.on("click", event => {
    if (state.measureMode) {
      addMeasurePoint(state, event.point.x, event.point.y, true);
    } else {
      selectAtClientPoint(state, event.point.x, event.point.y, true);
    }
  });
  map.getCanvas().addEventListener("contextmenu", event => {
    event.preventDefault();
    finishMeasure(state);
  });
  await onceMapEvent(map, "load");
  addThreeCustomLayer(state);
}

function addThreeCustomLayer(state) {
  const customLayer = {
    id: "ifc-three-3d-tiles",
    type: "custom",
    renderingMode: "3d",
    onAdd(map, gl) {
      state.camera = new THREE.PerspectiveCamera();
      state.tilesCamera = new THREE.PerspectiveCamera();
      state.renderer = new THREE.WebGLRenderer({
        canvas: map.getCanvas(),
        context: gl,
        antialias: true
      });
      state.renderer.autoClear = false;
      configureThreeRenderer(state);
      state.customLayerReady = true;
      if (state.tiles) {
        state.tiles.setCamera(state.tilesCamera);
        state.tiles.setResolutionFromRenderer(state.tilesCamera, state.renderer);
      }
    },
    render(gl, args) {
      if (state.tiles && state.tilesCamera) {
        state.tiles.setResolutionFromRenderer(state.tilesCamera, state.renderer);
        forceLoadGlbMaterialSliceTiles(state);
        state.tiles.update();
        forceLoadGlbMaterialSliceTiles(state);
        keepGlbMaterialSliceScenesVisible(state);
      }
      if (!state.camera || !state.renderer || !state.localTransform || !state.tilesCamera) {
        return;
      }
      syncMaplibreCamera(state, args);
      updateThreeCameraFillLight(state);
      state.renderer.resetState();
      state.renderer.render(state.scene, state.camera);
      updatePerformancePanel(state, performance.now());
      state.map.triggerRepaint();
    }
  };
  state.map.addLayer(customLayer);
}

function syncMaplibreCamera(state, args) {
  const renderMatrix = new THREE.Matrix4().fromArray(args.defaultProjectionData.mainMatrix);
  state.camera.projectionMatrix.copy(renderMatrix).multiply(state.localTransform);

  const projection = new THREE.Matrix4().fromArray(args.projectionMatrix);
  const inverseProjection = projection.clone().invert();
  const view = new THREE.Matrix4().multiplyMatrices(inverseProjection, state.camera.projectionMatrix);
  state.tilesCamera.projectionMatrix.copy(projection);
  state.tilesCamera.matrixWorldInverse.copy(view);
  state.tilesCamera.matrixWorld.copy(view).invert();
}

async function loadTileset(state, url) {
  const previousKey = state.selectedKey;
  clearSelection(state, false);
  if (state.tiles) {
    state.tiles.dispose();
    state.tiles = null;
  }

  const tiles = new TilesRenderer(url);
  tiles.group.name = "ifc-tiles";
  tiles.setCamera(state.tilesCamera || state.camera);
  if (state.renderer) {
    tiles.setResolutionFromRenderer(state.tilesCamera || state.camera, state.renderer);
  }
  tiles.errorTarget = 12;
  tiles.autoDisableRendererCulling = true;
  if (state.isGlbTilesOutput) {
    configureGlbMaterialSliceTiles(tiles);
  }
  tiles.fetchOptions.mode = "cors";
  state.tiles = tiles;
  state.scene.add(tiles.group);

  tiles.addEventListener("load-tileset", () => handleTilesetReady(state));
  tiles.addEventListener("load-model", event => handleTileModelLoaded(state, event));
  tiles.addEventListener("tile-download-start", event => {
    if (event.uri) {
      event.tile.userData = event.tile.userData || {};
      event.tile.userData.url = event.uri;
    }
  });
  tiles.addEventListener("load-content", () => {
    if (state.status) {
      state.status.textContent = "3D Tiles 載入中：" + TILESET_OPTIONS[state.normalModeIndex].label;
    }
  });

  if (previousKey) {
    state.selectedKey = previousKey;
    state.restoreSelectionPending = previousKey;
  }
  tiles.update();
}

function handleTilesetReady(state) {
  const sphere = new THREE.Sphere();
  if (!state.tiles.getBoundingSphere(sphere)) {
    return;
  }
  const rootTransform = state.tiles.root?.engineData?.transform;
  if (rootTransform) {
    state.rootOriginEcef.setFromMatrixPosition(rootTransform);
  } else {
    state.rootOriginEcef.copy(sphere.center);
  }
  state.rootOriginLngLatAlt = ecefToLngLatAlt(
    state.rootOriginEcef.x,
    state.rootOriginEcef.y,
    state.rootOriginEcef.z
  );
  state.modelCenter.set(0, 0, 0);

  if (state.mode === "maplibre") {
    configureMapLibreTilesTransform(state);
  } else {
    configurePureThreeTilesTransform(state);
    framePureThreeCamera(state, sphere.radius);
  }
  if (state.status) {
    state.status.textContent = "已載入：" + TILESET_OPTIONS[state.normalModeIndex].label;
  }
  forceLoadGlbMaterialSliceTiles(state);
}

function configureGlbMaterialSliceTiles(tiles) {
  tiles.displayActiveTiles = true;
  tiles.errorTarget = 0;
  if (tiles.lruCache) {
    tiles.lruCache.maxSize = Math.max(tiles.lruCache.maxSize || 0, 10000);
    tiles.lruCache.minSize = Math.max(tiles.lruCache.minSize || 0, 9000);
    tiles.lruCache.maxBytesSize = Infinity;
  }
}

function forceLoadGlbMaterialSliceTiles(state) {
  if (!state.isGlbTilesOutput || !state.tiles?.root?.children?.length) {
    return;
  }
  const tiles = state.tiles;
  if (typeof tiles.ensureChildrenArePreprocessed === "function") {
    tiles.ensureChildrenArePreprocessed(tiles.root, true);
  }
  for (const child of tiles.root.children) {
    if (!child.internal) {
      continue;
    }
    if (typeof tiles.markTileUsed === "function") {
      tiles.markTileUsed(child);
    }
    if (typeof tiles.queueTileForDownload === "function") {
      tiles.queueTileForDownload(child);
    }
  }
}

function configurePureThreeTilesTransform(state) {
  const enuFromEcef = getEnuFromEcefMatrix(state.rootOriginEcef);
  const moveToOrigin = new THREE.Matrix4().makeTranslation(
    -state.rootOriginEcef.x,
    -state.rootOriginEcef.y,
    -state.rootOriginEcef.z
  );
  const ecefToLocal = new THREE.Matrix4().multiplyMatrices(enuFromEcef, moveToOrigin);
  state.tiles.group.matrix.multiplyMatrices(getContentAxisMatrix(state), ecefToLocal);
  state.tiles.group.matrixAutoUpdate = false;
  state.tiles.group.updateMatrixWorld(true);
}

function configureMapLibreTilesTransform(state) {
  const coord = state.rootOriginLngLatAlt;
  if (!coord || !state.map) {
    return;
  }
  state.map.jumpTo({ center: [coord.lng, coord.lat], zoom: 18, pitch: 58 });
  state.localTransform = buildMaplibreLocalTransform(coord);
  const enuFromEcef = getEnuFromEcefMatrix(state.rootOriginEcef);
  const moveToOrigin = new THREE.Matrix4().makeTranslation(
    -state.rootOriginEcef.x,
    -state.rootOriginEcef.y,
    -state.rootOriginEcef.z
  );
  const ecefToLocal = new THREE.Matrix4().multiplyMatrices(enuFromEcef, moveToOrigin);
  state.tiles.group.matrix.multiplyMatrices(getContentAxisMatrix(state), ecefToLocal);
  state.tiles.group.matrixAutoUpdate = false;
  state.tiles.group.updateMatrixWorld(true);
}

function getContentAxisMatrix(state) {
  if (state.contentUpAxis === "y") {
    return new THREE.Matrix4().makeRotationX(-Math.PI / 2);
  }
  return new THREE.Matrix4().identity();
}

function buildMaplibreLocalTransform(coord) {
  const mercator = maplibregl.MercatorCoordinate.fromLngLat([coord.lng, coord.lat], coord.alt);
  const scale = mercator.meterInMercatorCoordinateUnits();
  const rotationX = new THREE.Matrix4().makeRotationAxis(new THREE.Vector3(1, 0, 0), Math.PI / 2);
  return new THREE.Matrix4()
    .makeTranslation(mercator.x, mercator.y, mercator.z)
    .scale(new THREE.Vector3(scale, -scale, scale))
    .multiply(rotationX);
}

function handleTileModelLoaded(state, event) {
  const tileUrl = event.url || event.tile?.userData?.url || "";
  if (tileUrl && !state.loadedTileUrls.has(tileUrl)) {
    state.loadedTileUrls.add(tileUrl);
  }
  if (tileUrl && event.scene.batchTable) {
    batchTableCache.set(tileUrl, {
      tileUrl,
      byteLength: 0,
      batchLength: event.scene.batchTable.count || 1,
      nativeBatchTable: event.scene.batchTable,
      batchTable: event.scene.batchTable.header || {}
    });
  }
  if (state.isGlbTilesOutput) {
    state.glbTileScenes.add(event.scene);
  }
  applyTileContentOrientation(state, event.scene);
  event.scene.userData.tileUrl = tileUrl;
  event.scene.userData.tile = event.tile;
  event.scene.traverse(child => {
    if (child.isMesh) {
      child.userData.tileUrl = tileUrl;
      child.userData.tile = event.tile;
      child.frustumCulled = false;
      applyGlbTileMaterialDisplay(state, child);
      rememberOriginalMaterial(state, child);
    }
  });
  applyFocusDisplay(state);
  keepGlbMaterialSliceScenesVisible(state);
  if (state.restoreSelectionPending) {
    restoreSelectionFromTile(state, event.scene, tileUrl);
  }
}

function keepGlbMaterialSliceScenesVisible(state) {
  if (!state.isGlbTilesOutput || !state.glbTileScenes.size) {
    return;
  }
  for (const scene of Array.from(state.glbTileScenes)) {
    if (!scene.parent) {
      state.glbTileScenes.delete(scene);
      continue;
    }
    scene.visible = true;
    scene.traverse(child => {
      if (child.isMesh) {
        child.frustumCulled = false;
      }
    });
  }
  updateSelectedBaseVisibility(state);
}

async function resolveContentOptions(state) {
  state.contentUpAxisMode = normalizeContentUpAxis(state.viewerConfig.contentUpAxis);
  state.doubleSidedMode = normalizeAutoBoolean(state.viewerConfig.doubleSided);
  const needsAuto = state.contentUpAxisMode === "auto" || state.doubleSidedMode === "auto";
  const glbReport = needsAuto ? await readGlbTilesReportOptions() : { isGlbOutput: false };
  state.isGlbTilesOutput = glbReport.isGlbOutput;
  const reportAxis = normalizeContentUpAxis(glbReport.viewerContentUpAxis);
  state.contentUpAxis = state.contentUpAxisMode === "auto"
    ? (reportAxis !== "auto" ? reportAxis : (glbReport.isGlbOutput ? "y" : "z"))
    : state.contentUpAxisMode;
  state.doubleSided = state.doubleSidedMode === "auto" ? glbReport.isGlbOutput : state.doubleSidedMode === true;
}

async function readGlbTilesReportOptions() {
  try {
    const response = await fetch("./glb_3dtiles_report.json", { cache: "no-store" });
    if (!response.ok) {
      return { isGlbOutput: false };
    }
    const report = await response.json().catch(() => ({}));
    return {
      isGlbOutput: true,
      viewerContentUpAxis: report.viewer_content_up_axis || report.viewerContentUpAxis || report.content_up_axis || report.contentUpAxis
    };
  } catch {
    return { isGlbOutput: false };
  }
}

function applyTileContentOrientation(state, scene) {
  if (state.contentUpAxis !== "y" || !scene || scene.userData.yUpToZUpApplied) {
    return;
  }
  scene.userData.yUpToZUpApplied = true;
  scene.updateMatrixWorld(true);
}

function applyGlbTileMaterialDisplay(state, mesh) {
  const materials = Array.isArray(mesh.material) ? mesh.material : [mesh.material];
  for (const material of materials) {
    if (!material) {
      continue;
    }
    if (state.doubleSided) {
      material.side = THREE.DoubleSide;
    }
    applyThreeReflectionMaterial(state, material);
    material.needsUpdate = true;
  }
}

function applyThreeReflectionMaterial(state, material) {
  if (state.reflectionBoost <= 0) {
    return;
  }
  if ("envMapIntensity" in material) {
    material.envMapIntensity = Math.max(material.envMapIntensity || 1, 1 + state.reflectionBoost * 2.2);
  }
  if ("roughness" in material) {
    material.roughness = Math.min(material.roughness == null ? 1 : material.roughness, state.reflectionRoughness);
  }
  if ("metalness" in material) {
    material.metalness = Math.max(material.metalness || 0, state.reflectionBoost * 0.08);
  }
}

async function restoreSelectionFromTile(state, scene, tileUrl) {
  const table = await getBatchTable(tileUrl);
  if (!table || !state.restoreSelectionPending) {
    return;
  }
  for (let batchId = 0; batchId < table.batchLength; batchId++) {
    const metadata = getFeatureMetadata(table, batchId);
    if (getSelectionKey(metadata, batchId, tileUrl) === state.restoreSelectionPending) {
      const mesh = findFirstMesh(scene);
      if (!mesh) {
        return;
      }
      const fakeIntersection = { object: mesh, faceIndex: 0, point: new THREE.Vector3() };
      mesh.getWorldPosition(fakeIntersection.point);
      await setSelectedFeature(state, fakeIntersection, metadata, batchId, tileUrl);
      state.restoreSelectionPending = null;
      return;
    }
  }
}

function bindToolbar(state) {
  const focusToggle = document.getElementById("focusToggle");
  const clearSelectionButton = document.getElementById("clearSelectionButton");
  const normalModeSlider = document.getElementById("normalModeSlider");
  const normalModeValue = document.getElementById("normalModeValue");
  const renderScaleSlider = document.getElementById("renderScaleSlider");
  const renderScaleValue = document.getElementById("renderScaleValue");
  const autoRenderScaleToggle = document.getElementById("autoRenderScaleToggle");
  const explodeSlider = document.getElementById("explodeSlider");
  const explodeValue = document.getElementById("explodeValue");
  const resetExplodeButton = document.getElementById("resetExplodeButton");
  const moveOutSlider = document.getElementById("moveOutSlider");
  const moveOutValue = document.getElementById("moveOutValue");
  const resetMoveOutButton = document.getElementById("resetMoveOutButton");
  const movePad = document.getElementById("movePad");
  const basemapSelect = document.getElementById("basemapSelect");
  const groundToggle = document.getElementById("groundToggle");
  const clearMeasureButton = document.getElementById("clearMeasureButton");

  focusToggle?.addEventListener("click", () => {
    state.focusEnabled = !state.focusEnabled;
    focusToggle.textContent = state.focusEnabled ? "焦點 ON" : "焦點 OFF";
    focusToggle.setAttribute("aria-pressed", state.focusEnabled ? "true" : "false");
    applyFocusDisplay(state);
    state.status.textContent = state.focusEnabled ? "焦點顯示開啟" : "焦點顯示關閉，選取仍可用";
  });
  clearSelectionButton?.addEventListener("click", () => clearSelection(state, true));
  normalModeSlider?.addEventListener("input", () => {
    const index = Math.max(0, Math.min(TILESET_OPTIONS.length - 1, Number(normalModeSlider.value) || 0));
    state.normalModeIndex = index;
    normalModeValue.textContent = TILESET_OPTIONS[index].label;
    loadTileset(state, TILESET_OPTIONS[index].url);
  });
  renderScaleSlider?.addEventListener("input", () => {
    state.renderScaleTarget = Number(renderScaleSlider.value);
    state.autoRenderScale = autoRenderScaleToggle?.checked ?? false;
    applyRenderScale(state, state.renderScaleTarget, true);
    updateRenderScaleUi(state);
  });
  autoRenderScaleToggle?.addEventListener("change", () => {
    state.autoRenderScale = autoRenderScaleToggle.checked;
    applyRenderScale(state, state.autoRenderScale ? AUTO_RENDER_SCALE_RESTORED : state.renderScaleTarget, true);
    updateRenderScaleUi(state);
  });
  explodeSlider?.addEventListener("input", () => {
    state.explode = Number(explodeSlider.value);
    explodeValue.textContent = state.explode.toFixed(0) + " m";
    applySelectedTransform(state);
  });
  resetExplodeButton?.addEventListener("click", () => {
    state.explode = 0;
    explodeSlider.value = "0";
    explodeValue.textContent = "0 m";
    applySelectedTransform(state);
  });
  moveOutSlider?.addEventListener("input", () => {
    state.moveOut = Number(moveOutSlider.value);
    moveOutValue.textContent = state.moveOut.toFixed(0) + " m";
    applySelectedTransform(state);
  });
  resetMoveOutButton?.addEventListener("click", () => {
    state.moveOut = 0;
    moveOutSlider.value = "0";
    moveOutValue.textContent = "0 m";
    setMoveDirection(state, "sw");
    applySelectedTransform(state);
  });
  movePad?.querySelectorAll("[data-move-direction]").forEach(button => {
    button.addEventListener("click", () => {
      setMoveDirection(state, button.dataset.moveDirection);
      applySelectedTransform(state);
    });
  });
  basemapSelect?.addEventListener("change", () => setBasemap(state, basemapSelect.value));
  groundToggle?.addEventListener("change", () => {
    state.groundVisible = groundToggle.checked;
    if (state.groundLayer) {
      state.groundLayer.visible = state.groundVisible;
    }
  });
  document.querySelectorAll("[data-measure-mode]").forEach(button => {
    button.addEventListener("click", () => setMeasureMode(state, button.dataset.measureMode));
  });
  clearMeasureButton?.addEventListener("click", () => clearMeasurements(state));

  applyInitialToolbarConfig(state);
}

function applyInitialToolbarConfig(state) {
  const focusToggle = document.getElementById("focusToggle");
  const normalModeSlider = document.getElementById("normalModeSlider");
  const normalModeValue = document.getElementById("normalModeValue");
  const renderScaleSlider = document.getElementById("renderScaleSlider");
  const autoRenderScaleToggle = document.getElementById("autoRenderScaleToggle");
  const explodeSlider = document.getElementById("explodeSlider");
  const explodeValue = document.getElementById("explodeValue");
  const moveOutSlider = document.getElementById("moveOutSlider");
  const moveOutValue = document.getElementById("moveOutValue");
  const basemapSelect = document.getElementById("basemapSelect");
  const groundToggle = document.getElementById("groundToggle");

  if (focusToggle) {
    focusToggle.textContent = state.focusEnabled ? "焦點 ON" : "焦點 OFF";
    focusToggle.setAttribute("aria-pressed", state.focusEnabled ? "true" : "false");
  }
  if (normalModeSlider) {
    normalModeSlider.value = String(state.normalModeIndex);
  }
  if (normalModeValue) {
    normalModeValue.textContent = TILESET_OPTIONS[state.normalModeIndex].label;
  }
  if (renderScaleSlider) {
    renderScaleSlider.value = state.renderScaleTarget.toFixed(2);
  }
  if (autoRenderScaleToggle) {
    autoRenderScaleToggle.checked = state.autoRenderScale;
  }
  if (explodeSlider) {
    explodeSlider.value = String(state.explode);
  }
  if (explodeValue) {
    explodeValue.textContent = state.explode.toFixed(0) + " m";
  }
  if (moveOutSlider) {
    moveOutSlider.value = String(state.moveOut);
  }
  if (moveOutValue) {
    moveOutValue.textContent = state.moveOut.toFixed(0) + " m";
  }
  if (basemapSelect) {
    basemapSelect.value = state.basemap;
  }
  if (groundToggle) {
    groundToggle.checked = state.groundVisible;
  }
  setMoveDirection(state, state.moveDirectionKey);
  updateRenderScaleUi(state);
}

function setMoveDirection(state, key) {
  state.moveDirectionKey = normalizeMoveDirection(key, "sw");
  const movePad = document.getElementById("movePad");
  movePad?.querySelectorAll("[data-move-direction]").forEach(button => {
    button.classList.toggle("is-active", button.dataset.moveDirection === state.moveDirectionKey);
  });
}

function setBasemap(state, key) {
  state.basemap = normalizeBasemap(key, state.basemap || "emap5");
  if (state.mode !== "maplibre" || !state.map) {
    state.status.textContent = state.mode === "maplibre"
      ? "底圖設定已記錄，地圖載入後套用"
      : "純 Three 版不載 WMTS，用地面/水面/格網排除底圖變因";
    return;
  }
  for (const id of Object.keys(BASEMAPS)) {
    if (state.map.getLayer("basemap-" + id)) {
      state.map.setLayoutProperty("basemap-" + id, "visibility", id === state.basemap ? "visible" : "none");
    }
  }
  state.status.textContent = "底圖：" + (BASEMAPS[state.basemap]?.label || state.basemap);
}

function setMeasureMode(state, mode) {
  state.measureMode = state.measureMode === mode ? "" : mode;
  state.measurePoints = [];
  rebuildMeasureLayer(state);
  document.querySelectorAll("[data-measure-mode]").forEach(button => {
    button.classList.toggle("is-active", button.dataset.measureMode === state.measureMode);
  });
  if (!state.measureMode) {
    state.status.textContent = "選取模式";
  } else if (state.measureMode === "distance") {
    state.status.textContent = "量距：點選兩點以上，右鍵完成";
  } else {
    state.status.textContent = "量面：點選三點以上，右鍵完成";
  }
}

async function selectAtClientPoint(state, clientX, clientY, mapPoint = false) {
  const hit = raycastTiles(state, clientX, clientY, mapPoint);
  if (!hit) {
    clearSelection(state, true);
    return;
  }
  const tileUrl = getTileUrlFromObject(hit.object);
  const table = await getBatchTable(tileUrl);
  const batchId = inferBatchId(hit);
  const metadata = table ? getFeatureMetadata(table, clampBatchId(batchId, table.batchLength), tileUrl) : {};
  await setSelectedFeature(state, hit, metadata, clampBatchId(batchId, table?.batchLength || 1), tileUrl);
}

async function setSelectedFeature(state, intersection, metadata, batchId, tileUrl) {
  clearSelection(state, false);
  const overlay = buildSelectedOverlay(state, intersection, batchId);
  if (!overlay) {
    state.status.textContent = "有點到，但無法建立選取 overlay";
    return;
  }
  overlay.userData.metadata = metadata;
  overlay.userData.batchId = batchId;
  overlay.userData.tileUrl = tileUrl;
  state.selectedOverlayGroup.add(overlay);
  state.selectedBaseMesh = intersection.object;
  state.selectedBaseWasVisible = intersection.object.visible;
  state.selected = {
    metadata,
    batchId,
    tileUrl,
    center: getObjectCenter(overlay),
    explodeDirection: getExplodeDirection(state, overlay)
  };
  state.selectedKey = getSelectionKey(metadata, batchId, tileUrl);
  updateSelectedBaseVisibility(state);
  applyFocusDisplay(state);
  applySelectedTransform(state);
  updateMetadataPanel(state, metadata, batchId, tileUrl);
  state.status.textContent = "已選取：" + formatFeatureName(metadata, batchId);
}

function buildSelectedOverlay(state, intersection, batchId) {
  const source = intersection.object;
  if (!source?.geometry) {
    return null;
  }
  const geometry = extractSelectionGeometry(source.geometry, batchId, intersection, state?.isGlbTilesOutput);
  if (!geometry) {
    return null;
  }
  geometry.applyMatrix4(source.matrixWorld);
  geometry.computeBoundingBox();
  geometry.computeBoundingSphere();
  const selectionBox = expandGlbSelectionBox(state, source, geometry.boundingBox.clone());
  const group = new THREE.Group();
  const border = buildSelectionBorder(selectionBox);
  border.name = "selected-feature-border";
  group.add(border);
  group.userData.sourceObject = source;
  return group;
}

function buildSelectionBorder(selectionBox) {
  const box = selectionBox.clone();
  const size = box.getSize(new THREE.Vector3());
  const padding = Math.max(0.04, size.length() * 0.003);
  box.expandByScalar(padding);
  const min = box.min;
  const max = box.max;
  const corners = [
    [min.x, min.y, min.z], [max.x, min.y, min.z], [max.x, max.y, min.z], [min.x, max.y, min.z],
    [min.x, min.y, max.z], [max.x, min.y, max.z], [max.x, max.y, max.z], [min.x, max.y, max.z]
  ];
  const edgePairs = [
    [0, 1], [1, 2], [2, 3], [3, 0],
    [4, 5], [5, 6], [6, 7], [7, 4],
    [0, 4], [1, 5], [2, 6], [3, 7]
  ];
  const positions = [];
  for (const [start, end] of edgePairs) {
    positions.push(...corners[start], ...corners[end]);
  }
  const borderGeometry = new THREE.BufferGeometry();
  borderGeometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  const material = new THREE.LineBasicMaterial({
    color: 0xffd21a,
    transparent: true,
    opacity: 0.98,
    depthTest: false,
    depthWrite: false
  });
  return new THREE.LineSegments(borderGeometry, material);
}

function expandGlbSelectionBox(state, source, seedBox) {
  if (!state?.isGlbTilesOutput || !source?.geometry) {
    return seedBox;
  }
  const box = seedBox.clone();
  const seedSize = seedBox.getSize(new THREE.Vector3());
  const longAxis = Math.max(seedSize.x, seedSize.y);
  const shortAxis = Math.min(seedSize.x, seedSize.y);
  if (seedSize.z < 1.0) {
    const padding = Math.min(8.0, Math.max(2.0, longAxis * 0.18));
    box.min.x -= padding;
    box.max.x += padding;
    box.min.y -= padding;
    box.max.y += padding;
    box.min.z = Math.min(box.min.z, 0);
    return box;
  }
  if (seedSize.z > 2.0 && longAxis > 1.0) {
    const padding = Math.min(5.0, Math.max(1.0, longAxis * 0.12));
    if (shortAxis < Math.max(1.0, longAxis * 0.18)) {
      if (seedSize.x < seedSize.y) {
        box.min.x -= padding;
        box.max.x += padding;
      } else {
        box.min.y -= padding;
        box.max.y += padding;
      }
    }
    box.min.z = Math.min(box.min.z, 0);
  }
  return box;
}

function extractSelectionGeometry(geometry, batchId, intersection, forceConnected = false) {
  if ((forceConnected || !getBatchAttribute(geometry)) && intersection?.faceIndex != null) {
    const connected = extractConnectedComponentGeometry(geometry, intersection.faceIndex);
    if (connected) {
      return connected;
    }
  }
  return extractFeatureGeometry(geometry, batchId);
}

function extractConnectedComponentGeometry(geometry, seedFaceIndex) {
  const position = geometry.getAttribute("position");
  if (!position) {
    return null;
  }
  const triangleCount = getTriangleCount(geometry);
  const seedFace = Math.floor(Number(seedFaceIndex));
  if (!Number.isFinite(seedFace) || seedFace < 0 || seedFace >= triangleCount) {
    return null;
  }

  const faceKeys = new Array(triangleCount);
  const facesByPosition = new Map();
  for (let face = 0; face < triangleCount; face++) {
    const keys = getTriangleVertexIds(geometry, face).map(vertexId => positionKey(position, vertexId));
    faceKeys[face] = keys;
    for (const key of new Set(keys)) {
      let faces = facesByPosition.get(key);
      if (!faces) {
        faces = [];
        facesByPosition.set(key, faces);
      }
      faces.push(face);
    }
  }

  const selectedFaces = new Set([seedFace]);
  const queue = [seedFace];
  for (let cursor = 0; cursor < queue.length; cursor++) {
    const face = queue[cursor];
    for (const key of faceKeys[face]) {
      const neighbours = facesByPosition.get(key) || [];
      for (const nextFace of neighbours) {
        if (!selectedFaces.has(nextFace)) {
          selectedFaces.add(nextFace);
          queue.push(nextFace);
        }
      }
    }
  }

  return extractGeometryFaces(geometry, selectedFaces);
}

function extractFeatureGeometry(geometry, batchId) {
  const position = geometry.getAttribute("position");
  if (!position) {
    return null;
  }
  const normal = geometry.getAttribute("normal");
  const color = geometry.getAttribute("color") || geometry.getAttribute("COLOR_0");
  const batchAttr = getBatchAttribute(geometry);
  const index = geometry.index;
  const positions = [];
  const normals = [];
  const colors = [];
  const triangleCount = index ? index.count / 3 : position.count / 3;
  let copiedTriangles = 0;
  const includeAll = !batchAttr || batchId == null || batchId < 0;

  for (let face = 0; face < triangleCount; face++) {
    const vertexIds = [0, 1, 2].map(offset => index ? index.getX(face * 3 + offset) : face * 3 + offset);
    if (!includeAll) {
      const match = vertexIds.some(vertexId => Number(batchAttr.getX(vertexId)) === Number(batchId));
      if (!match) {
        continue;
      }
    }
    for (const vertexId of vertexIds) {
      positions.push(position.getX(vertexId), position.getY(vertexId), position.getZ(vertexId));
      if (normal) {
        normals.push(normal.getX(vertexId), normal.getY(vertexId), normal.getZ(vertexId));
      }
      if (color) {
        colors.push(color.getX(vertexId), color.getY(vertexId), color.getZ(vertexId));
      }
    }
    copiedTriangles++;
  }

  if (copiedTriangles === 0 && !includeAll) {
    return extractFeatureGeometry(geometry, -1);
  }
  const result = new THREE.BufferGeometry();
  result.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  if (normals.length) {
    result.setAttribute("normal", new THREE.Float32BufferAttribute(normals, 3));
  }
  if (colors.length) {
    result.setAttribute("color", new THREE.Float32BufferAttribute(colors, 3));
  }
  return result;
}

function extractGeometryFaces(geometry, faceSet) {
  const position = geometry.getAttribute("position");
  if (!position || !faceSet?.size) {
    return null;
  }
  const normal = geometry.getAttribute("normal");
  const color = geometry.getAttribute("color") || geometry.getAttribute("COLOR_0");
  const positions = [];
  const normals = [];
  const colors = [];
  for (const face of faceSet) {
    for (const vertexId of getTriangleVertexIds(geometry, face)) {
      positions.push(position.getX(vertexId), position.getY(vertexId), position.getZ(vertexId));
      if (normal) {
        normals.push(normal.getX(vertexId), normal.getY(vertexId), normal.getZ(vertexId));
      }
      if (color) {
        colors.push(color.getX(vertexId), color.getY(vertexId), color.getZ(vertexId));
      }
    }
  }
  if (!positions.length) {
    return null;
  }
  const result = new THREE.BufferGeometry();
  result.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  if (normals.length) {
    result.setAttribute("normal", new THREE.Float32BufferAttribute(normals, 3));
  }
  if (colors.length) {
    result.setAttribute("color", new THREE.Float32BufferAttribute(colors, 3));
  }
  return result;
}

function getTriangleCount(geometry) {
  const position = geometry.getAttribute("position");
  return Math.floor((geometry.index ? geometry.index.count : (position?.count || 0)) / 3);
}

function getTriangleVertexIds(geometry, face) {
  const start = face * 3;
  if (geometry.index) {
    return [
      geometry.index.getX(start),
      geometry.index.getX(start + 1),
      geometry.index.getX(start + 2)
    ];
  }
  return [start, start + 1, start + 2];
}

function positionKey(position, vertexId) {
  const scale = SELECTION_POSITION_KEY_SCALE;
  return [
    Math.round(position.getX(vertexId) * scale),
    Math.round(position.getY(vertexId) * scale),
    Math.round(position.getZ(vertexId) * scale)
  ].join(",");
}

function inferBatchId(intersection) {
  const geometry = intersection.object.geometry;
  const batchAttr = getBatchAttribute(geometry);
  if (!batchAttr || intersection.faceIndex == null) {
    return 0;
  }
  const vertexIndex = geometry.index ? geometry.index.getX(intersection.faceIndex * 3) : intersection.faceIndex * 3;
  return Number(batchAttr.getX(vertexIndex)) || 0;
}

function getBatchAttribute(geometry) {
  return geometry.getAttribute("_BATCHID")
    || geometry.getAttribute("_batchid")
    || geometry.getAttribute("BATCHID")
    || geometry.getAttribute("batchid")
    || geometry.getAttribute("_FEATURE_ID_0")
    || geometry.getAttribute("featureId_0");
}

function applyFocusDisplay(state) {
  if (!state.tiles) {
    return;
  }
  state.tiles.group.traverse(child => {
    if (!child.isMesh) {
      return;
    }
    rememberOriginalMaterial(state, child);
    if (state.focusEnabled) {
      child.material = getFocusMaterial(child);
      child.visible = true;
    } else {
      child.material = state.baseMaterialState.get(child)?.material || child.material;
      child.visible = true;
    }
  });
  updateSelectedBaseVisibility(state);
}

function rememberOriginalMaterial(state, mesh) {
  if (!state.baseMaterialState.has(mesh)) {
    state.baseMaterialState.set(mesh, { material: mesh.material });
  }
}

function getFocusMaterial(mesh) {
  if (!mesh.userData.focusMaterial) {
    mesh.userData.focusMaterial = new THREE.MeshBasicMaterial({
      color: 0x7e98a8,
      transparent: true,
      opacity: 0.18,
      wireframe: true,
      depthWrite: false
    });
  }
  return mesh.userData.focusMaterial;
}

function updateSelectedBaseVisibility(state) {
  if (!state.selectedBaseMesh) {
    return;
  }
  const shouldHide = Boolean(state.selected && (state.explode > 0 || state.moveOut > 0 || state.focusEnabled));
  state.selectedBaseMesh.visible = shouldHide ? false : state.selectedBaseWasVisible;
}

function applySelectedTransform(state) {
  if (!state.selected || !state.selectedOverlayGroup) {
    if (state.explode > 0 || state.moveOut > 0) {
      state.status.textContent = "請先選取要移動的實體";
    }
    return;
  }
  const explodeOffset = state.selected.explodeDirection.clone().multiplyScalar(state.explode);
  const moveDirection = (MOVE_DIRECTIONS[state.moveDirectionKey] || MOVE_DIRECTIONS.sw).clone();
  const moveOffset = moveDirection.multiplyScalar(state.moveOut);
  state.selectedOverlayGroup.position.copy(explodeOffset.add(moveOffset));
  updateSelectedBaseVisibility(state);
}

function clearSelection(state, announce) {
  if (state.selectedBaseMesh) {
    state.selectedBaseMesh.visible = state.selectedBaseWasVisible;
  }
  state.selectedOverlayGroup.clear();
  state.selected = null;
  state.selectedBaseMesh = null;
  state.selectedKey = "";
  state.restoreSelectionPending = null;
  document.getElementById("metadataPanel").innerHTML = "<div class=\"empty\">未選取物件</div>";
  if (announce && state.status) {
    state.status.textContent = "已取消選取";
  }
}

function raycastTiles(state, clientX, clientY, mapPoint = false) {
  if (!state.tiles) {
    return null;
  }
  const canvas = state.mode === "maplibre" ? state.map.getCanvas() : state.renderer.domElement;
  const rect = canvas.getBoundingClientRect();
  const x = mapPoint ? clientX : clientX - rect.left;
  const y = mapPoint ? clientY : clientY - rect.top;
  state.pointer.x = (x / rect.width) * 2 - 1;
  state.pointer.y = -(y / rect.height) * 2 + 1;
  const camera = state.mode === "maplibre" ? state.tilesCamera : state.camera;
  if (!camera) {
    return null;
  }
  state.raycaster.setFromCamera(state.pointer, camera);
  state.raycaster.firstHitOnly = true;
  const hits = state.raycaster.intersectObject(state.tiles.group, true);
  return hits.find(hit => hit.object?.isMesh) || null;
}

function addMeasurePoint(state, clientX, clientY, mapPoint = false) {
  const point = getMeasurePoint(state, clientX, clientY, mapPoint);
  if (!point) {
    state.status.textContent = "沒有取得可量測位置";
    return;
  }
  state.measurePoints.push(point);
  rebuildMeasureLayer(state);
  if (state.measureMode === "distance") {
    const distance = getPolylineDistance(state.measurePoints);
    state.status.textContent = state.measurePoints.length < 2 ? "量距：請點第二點" : "量距：" + formatDistance(distance);
  } else if (state.measureMode === "area") {
    const area = getPlanarArea(state.measurePoints);
    state.status.textContent = state.measurePoints.length < 3 ? "量面：至少三點" : "量面：" + formatArea(area);
  }
}

function getMeasurePoint(state, clientX, clientY, mapPoint = false) {
  const hit = raycastTiles(state, clientX, clientY, mapPoint);
  if (hit) {
    return hit.point.clone();
  }
  if (state.mode === "three") {
    const rect = state.renderer.domElement.getBoundingClientRect();
    const pointer = new THREE.Vector2(
      ((clientX - rect.left) / rect.width) * 2 - 1,
      -((clientY - rect.top) / rect.height) * 2 + 1
    );
    const raycaster = new THREE.Raycaster();
    raycaster.setFromCamera(pointer, state.camera);
    const plane = new THREE.Plane(new THREE.Vector3(0, 0, 1), 0);
    const point = new THREE.Vector3();
    return raycaster.ray.intersectPlane(plane, point) ? point : null;
  }
  if (state.map) {
    const lngLat = state.map.unproject([clientX, clientY]);
    return lngLatToLocalEnu(state, lngLat.lng, lngLat.lat, 0);
  }
  return null;
}

function rebuildMeasureLayer(state) {
  state.measureLayer.clear();
  const points = state.measurePoints;
  if (points.length === 0) {
    return;
  }
  const markerMaterial = new THREE.MeshBasicMaterial({ color: 0xffd21a });
  for (const point of points) {
    const marker = new THREE.Mesh(new THREE.SphereGeometry(0.55, 12, 8), markerMaterial);
    marker.position.copy(point);
    state.measureLayer.add(marker);
  }
  if (points.length >= 2) {
    const linePoints = state.measureMode === "area" && points.length >= 3 ? [...points, points[0]] : points;
    const line = new THREE.Line(
      new THREE.BufferGeometry().setFromPoints(linePoints),
      new THREE.LineBasicMaterial({ color: 0xffd21a, linewidth: 2 })
    );
    state.measureLayer.add(line);
  }
}

function finishMeasure(state) {
  if (!state.measureMode) {
    return;
  }
  if (state.measureMode === "distance") {
    state.status.textContent = "量距完成：" + formatDistance(getPolylineDistance(state.measurePoints));
  } else {
    state.status.textContent = "量面完成：" + formatArea(getPlanarArea(state.measurePoints));
  }
  state.measureMode = "";
  document.querySelectorAll("[data-measure-mode]").forEach(button => button.classList.remove("is-active"));
}

function clearMeasurements(state) {
  state.measurePoints = [];
  state.measureMode = "";
  state.measureLayer.clear();
  document.querySelectorAll("[data-measure-mode]").forEach(button => button.classList.remove("is-active"));
  state.status.textContent = "量測已清除";
}

async function getBatchTable(tileUrl) {
  if (!tileUrl) {
    return null;
  }
  if (batchTableCache.has(tileUrl)) {
    return batchTableCache.get(tileUrl);
  }
  return null;
}

function parseB3dmBatchTable(buffer, tileUrl = "") {
  const view = new DataView(buffer);
  const magic = String.fromCharCode(
    view.getUint8(0),
    view.getUint8(1),
    view.getUint8(2),
    view.getUint8(3)
  );
  if (magic !== "b3dm") {
    return null;
  }
  const featureJsonLength = view.getUint32(12, true);
  const featureBinLength = view.getUint32(16, true);
  const batchJsonLength = view.getUint32(20, true);
  const batchBinLength = view.getUint32(24, true);
  let offset = 28;
  const featureJson = parsePaddedJson(buffer, offset, featureJsonLength);
  offset += featureJsonLength + featureBinLength;
  const batchJson = parsePaddedJson(buffer, offset, batchJsonLength);
  const batchLength = Number(featureJson?.BATCH_LENGTH || inferBatchLength(batchJson) || 1);
  return {
    tileUrl,
    byteLength: buffer.byteLength,
    batchLength,
    featureTable: featureJson,
    batchTable: batchJson || {},
    batchBinLength
  };
}

function parsePaddedJson(buffer, offset, length) {
  if (!length) {
    return {};
  }
  const bytes = new Uint8Array(buffer, offset, length);
  const text = textDecoder.decode(bytes).replace(/\0/g, "").trim();
  return text ? JSON.parse(text) : {};
}

function inferBatchLength(batchTable) {
  if (!batchTable) {
    return 0;
  }
  for (const value of Object.values(batchTable)) {
    if (Array.isArray(value)) {
      return value.length;
    }
  }
  return 0;
}

function getFeatureMetadata(table, batchId) {
  const result = { batch_id: batchId };
  if (table?.nativeBatchTable) {
    return table.nativeBatchTable.getDataFromId(batchId, result);
  }
  if (!table?.batchTable) {
    return result;
  }
  for (const [key, value] of Object.entries(table.batchTable)) {
    if (Array.isArray(value) && value.length === table.batchLength) {
      result[key] = value[batchId];
    } else {
      result[key] = value;
    }
  }
  if (result.batch_id == null) {
    result.batch_id = batchId;
  }
  return result;
}

function updateMetadataPanel(state, metadata, batchId, tileUrl) {
  const panel = document.getElementById("metadataPanel");
  if (!panel) {
    return;
  }
  const rows = [
    ["batch_id", batchId],
    ["ifc_step_id", metadata.ifc_step_id],
    ["global_id", metadata.global_id],
    ["ifc_type", metadata.ifc_type],
    ["name", metadata.name],
    ["description", metadata.description],
    ["dgn_element", metadata.dgn_element],
    ["site", metadata.site],
    ["building", metadata.building],
    ["storey", metadata.storey],
    ["group_names", metadata.group_names],
    ["style_id", metadata.style_id],
    ["color_rgba", metadata.color_rgba],
    ["psets_json", metadata.psets_json],
    ["tile", trimTileUrl(tileUrl)]
  ];
  panel.innerHTML = rows.map(([key, value]) => {
    const safeValue = escapeHtml(formatMetadataValue(value));
    return `<div class="meta-row"><span>${escapeHtml(key)}</span><strong title="${safeValue}">${safeValue}</strong></div>`;
  }).join("");
}

function formatMetadataValue(value) {
  if (value == null || value === "") {
    return "-";
  }
  const text = Array.isArray(value) || typeof value === "object" ? JSON.stringify(value) : String(value);
  return text.length > 280 ? text.slice(0, 280) + "..." : text;
}

function updatePerformancePanel(state, now) {
  const panel = document.getElementById("performancePanel");
  if (!panel || now - state.frame.lastPanelTime < 250) {
    return;
  }
  const delta = now - state.frame.lastTime;
  state.frame.frameMs = delta;
  state.frame.fps = delta > 0 ? 1000 / delta : 0;
  state.frame.lastTime = now;
  state.frame.lastPanelTime = now;
  const renderer = state.renderer;
  const stats = state.tiles?.stats || {};
  const loadingCount = Number(stats.queued || 0) + Number(stats.downloading || 0) + Number(stats.parsing || 0);
  if (loadingCount === 0 && state.status?.textContent?.startsWith("3D Tiles 載入中")) {
    state.status.textContent = "已載入：" + TILESET_OPTIONS[state.normalModeIndex].label;
  }
  const memory = renderer?.info?.memory || {};
  const render = renderer?.info?.render || {};
  const loadedBytes = getLoadedBytesEstimate(state);
  panel.innerHTML = [
    ["FPS", state.frame.fps.toFixed(0)],
    ["frame", state.frame.frameMs.toFixed(1) + " ms"],
    ["visible tiles", glbVisibleTileLabel(state, stats)],
    ["loaded tiles", stats.loaded ?? "-"],
    ["active tiles", stats.active ?? "-"],
    ["triangles", formatNumber(render.triangles || 0)],
    ["geometries", memory.geometries ?? "-"],
    ["textures", memory.textures ?? "-"],
    ["loaded bytes", loadedBytes ? formatBytes(loadedBytes) : "-"],
    ["pixel ratio", state.activeRenderScale.toFixed(2)],
    ["up axis", state.contentUpAxis + (state.contentUpAxisMode === "auto" ? " (auto)" : "")],
    ["double side", state.doubleSided ? "on" : "off"],
    ["selected", state.selectedKey || "-"],
    ["tile url", state.selected?.tileUrl ? trimTileUrl(state.selected.tileUrl) : "-"]
  ].map(([key, value]) => `<div class="perf-row"><span>${key}</span><strong>${escapeHtml(String(value))}</strong></div>`).join("");
}

function glbVisibleTileLabel(state, stats) {
  const visible = stats.visible ?? "-";
  if (!state.isGlbTilesOutput || !state.glbTileScenes.size) {
    return visible;
  }
  const guarded = Array.from(state.glbTileScenes).filter(scene => scene.parent).length;
  if (!guarded || guarded === visible) {
    return visible;
  }
  return `${visible} + ${guarded} guarded`;
}

function getLoadedBytesEstimate(state) {
  let total = 0;
  batchTableCache.forEach(item => {
    total += item?.byteLength || 0;
  });
  if (state.tiles?.lruCache?.itemSet) {
    state.tiles.lruCache.itemSet.forEach(tile => {
      total += state.tiles.lruCache.getMemoryUsage(tile) || 0;
    });
  }
  if (!total && state.tiles?.group) {
    state.tiles.group.traverse(child => {
      if (!child.geometry) {
        return;
      }
      for (const attribute of Object.values(child.geometry.attributes)) {
        total += attribute?.array?.byteLength || 0;
      }
      total += child.geometry.index?.array?.byteLength || 0;
    });
  }
  return total;
}

function animatePureThree(state) {
  requestAnimationFrame(() => animatePureThree(state));
  const now = performance.now();
  state.controls?.update();
  if (state.tiles) {
    state.tiles.setResolutionFromRenderer(state.camera, state.renderer);
    forceLoadGlbMaterialSliceTiles(state);
    state.tiles.update();
    forceLoadGlbMaterialSliceTiles(state);
    keepGlbMaterialSliceScenesVisible(state);
  }
  updateThreeCameraFillLight(state);
  state.renderer.render(state.scene, state.camera);
  updatePerformancePanel(state, now);
}

function updateThreeCameraFillLight(state) {
  if (!state.cameraFillLight || !state.camera) {
    return;
  }
  state.cameraFillLight.position.copy(state.camera.position);
}

function markInteraction(state) {
  beginRenderInteraction(state);
  endRenderInteraction(state);
}

function beginRenderInteraction(state) {
  if (!state.autoRenderScale) {
    return;
  }
  applyRenderScale(state, AUTO_RENDER_SCALE_MOVING, false);
  window.clearTimeout(state.restoreTimer);
}

function endRenderInteraction(state) {
  if (!state.autoRenderScale) {
    return;
  }
  window.clearTimeout(state.restoreTimer);
  state.restoreTimer = window.setTimeout(() => {
    applyRenderScale(state, AUTO_RENDER_SCALE_RESTORED, false);
  }, RESTORE_RENDER_SCALE_DELAY_MS);
}

function applyRenderScale(state, scale, fromUser) {
  const next = Number(scale) || AUTO_RENDER_SCALE_RESTORED;
  state.activeRenderScale = next;
  if (state.renderer) {
    state.renderer.setPixelRatio(window.devicePixelRatio * next);
    if (state.mode === "three") {
      state.renderer.setSize(rootWidth(state), rootHeight(state), false);
    }
  }
  // MapLibre 的 setPixelRatio 會同步觸發 resize/move 事件；自動畫質時容易形成遞迴。
  // 這裡只調 Three renderer，底圖維持 MapLibre 原生比例，讓比較重點留在 3D Tiles。
  if (state.debugScale) {
    console.log("Three viewer renderScale", next);
  }
  if (fromUser) {
    state.renderScaleTarget = next;
  }
  updateRenderScaleUi(state);
}

function updateRenderScaleUi(state) {
  const value = document.getElementById("renderScaleValue");
  if (value) {
    value.textContent = Math.round(state.activeRenderScale * 100) + "%";
  }
}

function resizePureThree(state) {
  if (!state.renderer || !state.camera) {
    return;
  }
  const width = rootWidth(state);
  const height = rootHeight(state);
  state.camera.aspect = width / height;
  state.camera.updateProjectionMatrix();
  state.renderer.setSize(width, height, false);
}

function framePureThreeCamera(state, radius) {
  const safeRadius = Math.max(80, Math.min(radius || 300, 1800));
  state.camera.position.set(safeRadius * 1.4, -safeRadius * 1.8, safeRadius * 0.9);
  state.camera.near = Math.max(0.1, safeRadius / 5000);
  state.camera.far = Math.max(5000, safeRadius * 20);
  state.camera.updateProjectionMatrix();
  state.controls.target.set(0, 0, 0);
  state.controls.update();
}

function createGroundLayer() {
  const group = new THREE.Group();
  group.name = "pure-three-ground";
  const water = new THREE.Mesh(
    new THREE.PlaneGeometry(2600, 2600, 1, 1),
    new THREE.MeshBasicMaterial({ color: 0x9fc8d4, transparent: true, opacity: 0.72, depthWrite: false })
  );
  water.position.z = -2;
  group.add(water);
  const grid = new THREE.GridHelper(2600, 52, 0x506d78, 0x83a0aa);
  grid.rotation.x = Math.PI / 2;
  grid.position.z = 0.03;
  group.add(grid);
  return group;
}

function buildMapLibreStyle(activeKey) {
  const sources = {};
  const layers = [
    { id: "background", type: "background", paint: { "background-color": "#9fc8d4" } }
  ];
  for (const [key, config] of Object.entries(BASEMAPS)) {
    sources["basemap-" + key] = {
      type: "raster",
      tiles: config.tiles,
      tileSize: config.tileSize,
      attribution: config.attribution
    };
    layers.push({
      id: "basemap-" + key,
      type: "raster",
      source: "basemap-" + key,
      layout: { visibility: key === activeKey ? "visible" : "none" },
      paint: { "raster-opacity": 1 }
    });
  }
  return { version: 8, sources, layers };
}

function onceMapEvent(map, name) {
  return new Promise(resolve => map.once(name, resolve));
}

function ecefToLngLatAlt(x, y, z) {
  const a = 6378137.0;
  const e2 = 6.69437999014e-3;
  const b = a * Math.sqrt(1 - e2);
  const ep2 = (a * a - b * b) / (b * b);
  const p = Math.sqrt(x * x + y * y);
  const th = Math.atan2(a * z, b * p);
  const lon = Math.atan2(y, x);
  const lat = Math.atan2(z + ep2 * b * Math.pow(Math.sin(th), 3), p - e2 * a * Math.pow(Math.cos(th), 3));
  const n = a / Math.sqrt(1 - e2 * Math.sin(lat) * Math.sin(lat));
  const alt = p / Math.cos(lat) - n;
  return {
    lng: lon * 180 / Math.PI,
    lat: lat * 180 / Math.PI,
    alt
  };
}

function lngLatAltToEcef(lng, lat, alt = 0) {
  const a = 6378137.0;
  const e2 = 6.69437999014e-3;
  const lon = lng * Math.PI / 180;
  const phi = lat * Math.PI / 180;
  const sinLat = Math.sin(phi);
  const cosLat = Math.cos(phi);
  const n = a / Math.sqrt(1 - e2 * sinLat * sinLat);
  return new THREE.Vector3(
    (n + alt) * cosLat * Math.cos(lon),
    (n + alt) * cosLat * Math.sin(lon),
    (n * (1 - e2) + alt) * sinLat
  );
}

function getEnuFromEcefMatrix(origin) {
  const coord = ecefToLngLatAlt(origin.x, origin.y, origin.z);
  const lon = coord.lng * Math.PI / 180;
  const lat = coord.lat * Math.PI / 180;
  const sinLon = Math.sin(lon);
  const cosLon = Math.cos(lon);
  const sinLat = Math.sin(lat);
  const cosLat = Math.cos(lat);
  const east = new THREE.Vector3(-sinLon, cosLon, 0);
  const north = new THREE.Vector3(-sinLat * cosLon, -sinLat * sinLon, cosLat);
  const up = new THREE.Vector3(cosLat * cosLon, cosLat * sinLon, sinLat);
  return new THREE.Matrix4().set(
    east.x, east.y, east.z, 0,
    north.x, north.y, north.z, 0,
    up.x, up.y, up.z, 0,
    0, 0, 0, 1
  );
}

function lngLatToLocalEnu(state, lng, lat, alt = 0) {
  const ecef = lngLatAltToEcef(lng, lat, alt);
  const matrix = getEnuFromEcefMatrix(state.rootOriginEcef);
  const relative = ecef.sub(state.rootOriginEcef);
  return relative.applyMatrix4(matrix);
}

function getTileUrlFromObject(object) {
  let current = object;
  while (current) {
    if (current.userData?.tileUrl) {
      return current.userData.tileUrl;
    }
    current = current.parent;
  }
  return "";
}

function findFirstMesh(object) {
  let found = null;
  object.traverse(child => {
    if (!found && child.isMesh) {
      found = child;
    }
  });
  return found;
}

function getObjectCenter(object) {
  const box = new THREE.Box3().setFromObject(object);
  return box.getCenter(new THREE.Vector3());
}

function getExplodeDirection(state, overlay) {
  const center = getObjectCenter(overlay);
  const direction = center.clone().sub(state.modelCenter);
  direction.z *= 0.35;
  if (direction.lengthSq() < 0.001) {
    direction.set(0, 0, 1);
  }
  return direction.normalize();
}

function getPolylineDistance(points) {
  let total = 0;
  for (let i = 1; i < points.length; i++) {
    total += points[i - 1].distanceTo(points[i]);
  }
  return total;
}

function getPlanarArea(points) {
  if (points.length < 3) {
    return 0;
  }
  let area = 0;
  for (let i = 0; i < points.length; i++) {
    const a = points[i];
    const b = points[(i + 1) % points.length];
    area += a.x * b.y - b.x * a.y;
  }
  return Math.abs(area) / 2;
}

function formatDistance(value) {
  return value >= 1000 ? (value / 1000).toFixed(3) + " km" : value.toFixed(2) + " m";
}

function formatArea(value) {
  return value >= 1000000 ? (value / 1000000).toFixed(3) + " km²" : value.toFixed(2) + " m²";
}

function formatBytes(value) {
  if (value > 1024 * 1024 * 1024) {
    return (value / 1024 / 1024 / 1024).toFixed(2) + " GB";
  }
  if (value > 1024 * 1024) {
    return (value / 1024 / 1024).toFixed(1) + " MB";
  }
  return (value / 1024).toFixed(1) + " KB";
}

function formatNumber(value) {
  return new Intl.NumberFormat("zh-TW").format(value);
}

function clampBatchId(batchId, batchLength) {
  const length = Math.max(1, Number(batchLength) || 1);
  return Math.max(0, Math.min(length - 1, Number(batchId) || 0));
}

function getSelectionKey(metadata, batchId, tileUrl) {
  return String(metadata.global_id || metadata.ifc_step_id || `${trimTileUrl(tileUrl)}#${batchId}`);
}

function formatFeatureName(metadata, batchId) {
  return metadata.name || metadata.ifc_type || metadata.ifc_step_id || ("batch " + batchId);
}

function trimTileUrl(tileUrl) {
  return String(tileUrl || "").replace(location.origin + location.pathname.replace(/\/[^/]*$/, "/"), "./");
}

function rootWidth(state) {
  return Math.max(320, state.root.clientWidth);
}

function rootHeight(state) {
  return Math.max(240, state.root.clientHeight);
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, char => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    "\"": "&quot;",
    "'": "&#039;"
  }[char]));
}
