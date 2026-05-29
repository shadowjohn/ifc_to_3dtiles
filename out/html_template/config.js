window.IFC_VIEWER_CONFIG = {
  // Shared defaults used by index.html, index_three.html, and index_maplibre_three.html.
  common: {
    basemap: "emap5",              // emap5 | googleRoadmap | googleSatellite | osm
    normalMode: "flat",            // flat | smooth90 | smooth
    renderScale: 1.0,              // 0.5 .. 1.0
    autoRenderScale: true,
    visualPreset: "sketchfab",     // default | sketchfab
    focus: false,
    explode: 0,                    // Cesium: percent 0..100; Three: meters
    moveOut: 0,                    // Cesium: percent 0..100; Three: meters
    moveDirection: "sw"            // n | ne | e | se | s | sw | w | nw | center
  },

  cesium: {
    // auto: GLB-derived tiles enable terrain/water pass-through to avoid close-view occlusion.
    // Use true or false to force a specific behavior.
    underground: "auto",
    closeViewDepthGuard: true,
    backgroundColor: "#dddddd",
    globeBaseColor: "#dddddd",
    lightIntensity: 3.2,
    imageBasedLighting: 2.35,
    modelLightBoost: 1.35,
    reflectionBoost: 0.38,
    reflectionRoughness: 0.28,
    cameraFillLight: true,
    cameraFillLightIntensity: 3.4,
    // Hue-preserving fill light for Cesium to better match Sketchfab environment lighting.
    ambientFill: 0.24,
    toneMapping: "filmic",
    displayBrightness: 0,
    displayContrast: 1.02,
    // Cesium shadowMap.darkness is the minimum light kept in shadow; higher means softer/lighter shadows.
    shadowDarkness: 0.68,
    shadowLift: 0,
    materialMode: "original",      // original | silver | dark | mirror
    atmosphere: false,
    sun: false,
    shadow: false,
    water: false,
    time: "auto"                   // auto | HH:mm | minutes 0..1439
  },

  three: {
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
    // auto: follows glb_3dtiles_report.json when available; legacy GLB output falls back to Y-up.
    // Use "y" or "z" to force a model-specific choice.
    contentUpAxis: "auto",
    // auto: GLB-derived tiles use DoubleSide to avoid missing walls from back-face culling.
    // Use true or false to force it.
    doubleSided: "auto"
  },

  maplibreThree: {
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
    // auto: follows glb_3dtiles_report.json when available; legacy GLB output falls back to Y-up.
    contentUpAxis: "auto",
    doubleSided: "auto"
  }
};
