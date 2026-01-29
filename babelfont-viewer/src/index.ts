import "./styles.css";
import { Font } from "babelfont";
import { NodeType, ReviverFunc } from "babelfont/dist/underlying";
import { customRegistry, MyAxis, MyPath } from "./registry";
import $ from "jquery";

let currentFont: Font | null = null;
let currentGlyphIndex: number | null = null;
let currentLayerIndex: number | null = null;
let canvasZoom: number = 1;
let cachedCanvasWidth: number = 0;
let cachedCanvasHeight: number = 0;

function stringifyLocation(loc: any): string {
  if (!loc) return "";
  return Object.entries(loc)
    .map(([k, v]) => `<code>${k}</code>=${v}`)
    .join(" | ");
}

function setStatus(text: string) {
  $("#status").text(text);
}

function formatCodepoints(cps?: number[]): string {
  if (!cps || cps.length === 0) return "";
  return cps
    .map((cp) => `U+${cp.toString(16).toUpperCase().padStart(4, "0")}`)
    .join(", ");
}

function renderGlyphList() {
  const list = $("#glyphList");
  list.empty();
  if (!currentFont) return;
  currentFont.glyphs.forEach((g, idx) => {
    const li = $("<li>");
    li.text(
      `${g.name}${g.codepoints && g.codepoints.length ? " — " + formatCodepoints(g.codepoints) : ""}`,
    );
    if (idx === currentGlyphIndex) {
      li.css({
        backgroundColor: "#4a9eff",
        color: "#fff",
        fontWeight: "600",
      });
    }
    li.on("click", () => {
      currentGlyphIndex = idx;
      renderGlyphList();
      renderLayerSelect();
      renderMetadata();
    });
    list.append(li);
  });
}

function renderLayerSelect() {
  const sel = $("#layerSelect") as JQuery<HTMLSelectElement>;
  sel.empty();
  const canvas = $("#layerCanvas")[0] as HTMLCanvasElement;
  const ctx = canvas.getContext("2d")!;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  canvasZoom = 1; // Reset zoom on layer change
  cachedCanvasWidth = 0; // Reset cached size on layer change
  cachedCanvasHeight = 0;
  if (currentGlyphIndex === null || !currentFont) return;
  const glyph = currentFont.glyphs[currentGlyphIndex];
  const layers = glyph.layers || [];
  layers.forEach((layer, idx) => {
    const opt = $("<option>");
    opt.val(String(idx));
    opt.text(layer.name || `Layer ${idx + 1}`);
    sel.append(opt);
  });
  sel.on("change", (e) => {
    const t = e.target as HTMLSelectElement;
    currentLayerIndex = parseInt(t.value, 10);
    canvasZoom = 1;
    updateLayerLocationInfo();
    updateLayerJSON();
    drawCurrentLayer();
    showLayerTab("render");
  });
  if (layers.length > 0) {
    sel.val("0");
    currentLayerIndex = 0;
    updateLayerLocationInfo();
    updateLayerJSON();
    drawCurrentLayer();
  }
}

function updateLayerLocationInfo() {
  const locInfo = $("#layerLocationInfo");
  locInfo.empty();
  if (currentGlyphIndex === null || currentLayerIndex === null || !currentFont)
    return;
  const layer =
    currentFont.glyphs[currentGlyphIndex].layers?.[currentLayerIndex];
  if (!layer) return;
  const info: string[] = [];
  if (layer.location && Object.keys(layer.location).length > 0) {
    info.push(
      `<tr><th>Location</th><td>${stringifyLocation(layer.location)}</td></tr>`,
    );
  }
  if (
    layer.smart_component_location &&
    Object.keys(layer.smart_component_location).length > 0
  ) {
    info.push(
      `<tr><th>Smart Component Location</th><td>${stringifyLocation(layer.smart_component_location)}</td></tr>`,
    );
  }
  if (info.length > 0) {
    locInfo.html(info.join(""));
    locInfo.css({
      fontSize: "12px",
      color: "#666",
      padding: "4px 6px",
    });
  }
}

function updateLayerJSON() {
  const jsonPre = $("#layerJSON");
  if (currentGlyphIndex === null || currentLayerIndex === null || !currentFont)
    return;
  const layer =
    currentFont.glyphs[currentGlyphIndex].layers?.[currentLayerIndex];
  if (!layer) return;
  jsonPre.text(JSON.stringify(layer, null, 2));
}

function showLayerTab(tab: "render" | "json") {
  const renderView = $("#selectedLayer");
  const jsonView = $("#layerJSON");
  const tabs = document.querySelectorAll("[data-layer-tab]");
  tabs.forEach((t) => t.classList.remove("active"));
  document.querySelector(`[data-layer-tab="${tab}"]`)?.classList.add("active");
  if (tab === "render") {
    renderView.css("display", "block");
    jsonView.css("display", "none");
  } else {
    renderView.css("display", "none");
    jsonView.css("display", "block");
  }
}

type Pt = { x: number; y: number };

function drawCurrentLayer() {
  const canvas = $("#layerCanvas")[0] as HTMLCanvasElement;
  const ctx = canvas.getContext("2d")!;
  const dpr = window.devicePixelRatio;

  // Cache the canvas logical size on first draw, then use it for all subsequent draws
  // This prevents the canvas size from changing due to rendering artifacts during zoom
  if (cachedCanvasWidth === 0 || cachedCanvasHeight === 0) {
    cachedCanvasWidth = canvas.clientWidth * dpr;
    cachedCanvasHeight = canvas.clientHeight * dpr;
  }

  const logicalWidth = cachedCanvasWidth;
  const logicalHeight = cachedCanvasHeight;

  // Set the "actual" size of the canvas (pixel resolution)
  canvas.width = logicalWidth;
  canvas.height = logicalHeight;

  // Scale the context to ensure correct drawing operations
  ctx.scale(dpr, dpr);

  // Black background (draw at logical size)
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = "#000";
  ctx.fillRect(0, 0, logicalWidth, logicalHeight);
  ctx.restore();

  if (currentGlyphIndex === null || currentLayerIndex === null || !currentFont)
    return;
  const layer =
    currentFont.glyphs[currentGlyphIndex].layers?.[currentLayerIndex];
  const shapes = layer?.shapes || [];

  // Compute bounds from all path nodes
  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;
  const paths: MyPath[] = [];
  for (const sh of shapes as any[]) {
    if (sh && "nodes" in sh) {
      const p = sh as MyPath;
      paths.push(p);
      (p.nodes || []).forEach((n) => {
        if (n.x < minX) minX = n.x;
        if (n.y < minY) minY = n.y;
        if (n.x > maxX) maxX = n.x;
        if (n.y > maxY) maxY = n.y;
      });
    }
  }
  if (!paths.length) return;

  const margin = 20;
  const w = Math.max(1, maxX - minX);
  const h = Math.max(1, maxY - minY);
  const sx = (logicalWidth - margin * 2) / w;
  const sy = (logicalHeight - margin * 2) / h;
  let s = Math.max(0.001, Math.min(sx, sy));
  s *= canvasZoom; // Apply zoom
  const centerX = logicalWidth / 2;
  const centerY = logicalHeight / 2;
  const glyphCenterX = minX + w / 2;
  const glyphCenterY = minY + h / 2;
  const tx = centerX - glyphCenterX * s;
  const ty = centerY + glyphCenterY * s; // account for Y flip

  // Set transform: scale and flip Y
  ctx.setTransform(s, 0, 0, -s, tx, ty);
  ctx.lineWidth = 1 / s;
  ctx.strokeStyle = "#fff";
  ctx.fillStyle = "transparent";

  // Draw baseline
  ctx.strokeStyle = "rgb(253, 155, 155)";
  ctx.beginPath();
  ctx.moveTo(0, 0);
  ctx.lineTo(layer.width, 0);
  ctx.stroke();
  let layerMasterId = layer.master?.master;
  let layerMaster =
    layerMasterId && currentFont.masters?.find((m) => m.id === layerMasterId);
  if (layerMaster) {
    // Draw ascender and descender
    ctx.strokeStyle = "rgb(155, 253, 155)";
    ctx.beginPath();
    const ascender = layerMaster.metrics.Ascender;
    if (ascender !== undefined) {
      ctx.moveTo(0, 0);
      ctx.lineTo(0, ascender);
      ctx.lineTo(layer.width, ascender);
      ctx.lineTo(layer.width, 0);
    }
    const descender = layerMaster.metrics.Descender;
    if (descender !== undefined) {
      ctx.moveTo(0, 0);
      ctx.lineTo(0, descender);
      ctx.lineTo(layer.width, descender);
      ctx.lineTo(layer.width, 0);
    }
    ctx.stroke();
  }

  ctx.strokeStyle = "#fff";

  // Draw main paths
  for (const p of paths) {
    const svg = p.toSvgPathString();
    if (!svg) continue;
    const path2d = new Path2D(svg);
    ctx.stroke(path2d);
  }

  // Draw helpers: control points and lines
  ctx.strokeStyle = "#07f";
  ctx.fillStyle = "transparent";
  const rOn = 8 / s;
  const rOff = 6 / s;
  const sq = 12 / s;

  for (const p of paths) {
    const nodes = p.nodes || [];
    if (nodes.length === 0) continue;
    for (let i = 0; i < nodes.length; i++) {
      const n = nodes[i];
      const type = n.nodetype;
      if (type === "OffCurve") {
        // draw small off-curve dot
        drawCircle(ctx, n.x, n.y, rOff);
      } else if (type === "Line") {
        // square at line node
        drawSquare(ctx, n.x, n.y, sq);
      } else if (type === "Curve") {
        // circle at curve endpoint
        drawCircle(ctx, n.x, n.y, rOn);
        const prevNode = nodes[i - 1 >= 0 ? i - 1 : 0];
        if (prevNode && prevNode.nodetype === NodeType.OffCurve) {
          // lines connecting this node to second CP
          ctx.beginPath();
          ctx.moveTo(n.x, n.y);
          ctx.lineTo(prevNode.x, prevNode.y);
          ctx.stroke();
        }
        const nextNode = nodes[i + 1 < nodes.length ? i + 1 : nodes.length - 1];
        if (nextNode && nextNode.nodetype === NodeType.OffCurve) {
          // lines connecting this node to first CP
          ctx.beginPath();
          ctx.moveTo(n.x, n.y);
          ctx.lineTo(nextNode.x, nextNode.y);
          ctx.stroke();
        }
      }
    }
  }
}

function drawCircle(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  r: number,
) {
  ctx.fillStyle = "blue";
  ctx.beginPath();
  ctx.arc(x, y, r, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = "transparent";
}

function drawSquare(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  size: number,
) {
  ctx.fillStyle = "blue";
  ctx.beginPath();
  ctx.rect(x - size / 2, y - size / 2, size, size);
  ctx.fill();
  ctx.fillStyle = "transparent";
}

function renderMetadata() {
  const content = $("#tabContent");
  content.empty();
  if (!currentFont) return;
  const activeTab = $(".tab.active");
  const tab = activeTab?.data("tab");
  if (tab === "names") {
    const table = $("<table>");
    for (const [key, value] of Object.entries(currentFont.names)) {
      const tr = $("<tr>");
      const th = $("<th>");
      th.text(titleCase(key.replace("_", " ")) || "");
      const td = $("<td>");
      td.text(value.dflt);
      tr.append(th);
      tr.append(td);
      table.append(tr);
    }
    content.append(table);
  } else if (tab === "masters") {
    const pre = $("<pre>");
    pre.text(JSON.stringify(currentFont.masters || [], null, 2));
    content.append(pre);
  } else if (tab === "axes") {
    renderAxes(content, currentFont);
  }
}

function renderAxes(container: JQuery<HTMLElement>, currentFont: Font) {
  const table = $("<table id='axesTable'>");
  const header = $("<tr>");
  header.html(
    `<th>Tag</th><th>Name</th><th>Min</th><th>Default</th><th>Max</th>`,
  );
  table.append(header);
  (currentFont.axes || []).forEach((axis) => {
    const a = axis as unknown as MyAxis; // show custom methods
    const row = $("<tr>");
    row.html(
      `<td><code>${a.tag}</code></td><td>${a.getDisplayName()}</td><td>${a.min}</td><td>${a.default}</td><td>${a.max}</td>`,
    );
    table.append(row);
  });
  container.append(table);
}
function wireTabs() {
  $(".tab").on("click", (el) => {
    $(".tab").removeClass("active");
    $(el.target).addClass("active");
    renderMetadata();
  });
}

function wireFileInput() {
  const input = $("#fileInput") as JQuery<HTMLInputElement>;
  input.on("change", async () => {
    const file = input[0].files?.[0];
    if (!file) return;
    loadFile(file);
  });
}

function main() {
  wireTabs();
  wireFileInput();
  wireCanvasZoom();
  wireLayerTabs();
  wireDividers();
  wireFileDragDrop();
}

function wireCanvasZoom() {
  const canvas = $("#layerCanvas")[0] as HTMLCanvasElement;
  canvas.addEventListener("wheel", (e: WheelEvent) => {
    e.preventDefault();
    const zoomDelta = e.deltaY > 0 ? 0.9 : 1.1;
    canvasZoom = Math.max(0.25, Math.min(4, canvasZoom * zoomDelta));
    drawCurrentLayer();
  });
}

function wireLayerTabs() {
  $("[data-layer-tab]").on("click", (el) => {
    const tab = $(el.target).attr("data-layer-tab") as "render" | "json";
    showLayerTab(tab);
  });
}

function wireDividers() {
  // Vertical divider (between glyphs and layers)
  const divV = $("#dividerVertical")[0] as HTMLElement;
  const appDiv = $("#app")[0] as HTMLElement;
  divV.addEventListener("mousedown", (e: MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const top = document.querySelector(".top") as HTMLElement;
    const startCol1 = parseFloat(
      getComputedStyle(top).gridTemplateColumns.split(" ")[0],
    );

    const handleMouseMove = (moveEvent: MouseEvent) => {
      const deltaX = moveEvent.clientX - startX;
      const newCol1 = startCol1 + deltaX;
      if (newCol1 > 100 && window.innerWidth - newCol1 > 100) {
        top.style.gridTemplateColumns = `${newCol1}px auto 1fr`;
      }
    };

    const handleMouseUp = () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
  });

  // Horizontal divider (between top and metadata)
  const divH = $("#dividerHorizontal")[0] as HTMLElement;
  divH.addEventListener("mousedown", (e: MouseEvent) => {
    e.preventDefault();
    const startY = e.clientY;
    const rows = getComputedStyle(appDiv).gridTemplateRows.split(" ");
    const startRow2 = parseFloat(rows[1]);

    const handleMouseMove = (moveEvent: MouseEvent) => {
      const deltaY = moveEvent.clientY - startY;
      const newRow2 = startRow2 + deltaY;
      if (newRow2 > 100 && window.innerHeight - newRow2 > 100) {
        appDiv.style.gridTemplateRows = `${rows[0]} ${newRow2}px ${rows[2]} 1fr`;
      }
    };

    const handleMouseUp = () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
  });
}

function wireFileDragDrop() {
  const appDiv = $("#app")[0] as HTMLElement;

  appDiv.addEventListener("dragover", (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    appDiv.style.backgroundColor = "rgba(74, 158, 255, 0.1)";
  });

  appDiv.addEventListener("dragleave", (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    appDiv.style.backgroundColor = "";
  });

  appDiv.addEventListener("drop", (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    appDiv.style.backgroundColor = "";

    const files = e.dataTransfer?.files;
    if (!files || files.length === 0) return;

    const file = files[0];
    if (!(file.name.endsWith(".babelfont") || file.name.endsWith(".json"))) {
      setStatus("Please drop a .babelfont or .json file");
      return;
    }

    loadFile(file);
  });
}

async function loadFile(file: File) {
  setStatus(`Loading ${file.name}...`);
  const text = await file.text();
  let raw: any;
  try {
    raw = JSON.parse(text, ReviverFunc);
  } catch (e) {
    setStatus("Invalid JSON file");
    console.error(e);
    return;
  }
  try {
    currentFont = new Font(raw, customRegistry);
    setStatus(`Loaded font with ${currentFont.glyphs.length} glyphs`);
    renderGlyphList();
    renderMetadata();
  } catch (e) {
    setStatus("Failed to parse Babelfont");
    console.error(e);
  }
}

document.addEventListener("DOMContentLoaded", main);
function titleCase(arg0: string): string | null {
  return arg0.replace(
    /\w\S*/g,
    (txt) => txt.charAt(0).toUpperCase() + txt.substr(1).toLowerCase(),
  );
}
