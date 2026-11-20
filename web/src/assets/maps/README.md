# Built-in ECharts maps

These maps are bundled for on-demand registration by `src/lib/echartsMaps.ts`. They are
loaded with Vite dynamic `import()`, so their GeoJSON stays out of the first-screen bundle.

## `china.json`

- Source: [GeoJSON.cn China atlas](https://geojson.cn/data/atlas/china), version `1.6.3`
  (2025-12-14), copyright GeoJSON.CN.
- Coverage: 34 province-level features with short standard Chinese names (`北京`, `广东`,
  `内蒙古`, `新疆`, `台湾`, `香港`, `澳门`, ...) plus the South China Sea line feature.
- Territorial completeness: Hainan's geometry includes the South China Sea islands and extends
  below 17°N; the line feature covers the full South China Sea. The upstream dataset labels the
  current line `十段线`; its source page documents the same dataset as the nine-dash line
  (`九段线`, also called `十段线` in current releases).
- Do not replace this file with a cropped mainland-only extract. The inset and southern
  coordinates are intentional and required for standard-map compliance.

## `world.json`

- Source: Apache ECharts `4.9.0`, `map/json/world.json` (BSD-3-Clause map data from Baidu Inc.).
- Country names follow ECharts conventions, including `China`, `United States`, `Russia`,
  `United Kingdom`, `Japan` and `France`.

## Runtime contract

`extractRequiredMaps` reads `requirements.required_maps`, `geo.map` and `series[*].map`.
`ensureEchartsMaps` registers only `china` and `world`, caches concurrent loads and returns any
unknown map names (for example `guangdong`) so the chart can show the extra-registration warning.
