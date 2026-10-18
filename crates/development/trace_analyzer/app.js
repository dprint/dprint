// @ts-check
// <reference path="types.d.ts" />

const traceResult = getTransformedTraceResult();

function onLoad() {
  const appData = {
    traceIndex: traceResult.traces.length - 1,
    selectedNodeId: traceResult.traces[traceResult.traces.length - 1].printNodeId,
  };

  const slider = createSlider(value => {
    appData.traceIndex = value;
    appData.selectedNodeId = traceResult.traces[value].printNodeId;
    refreshApp();
  });
  const codeView = createCodeView();
  const infoArea = createInfoArea();
  const graph = createGraph(printNodeId => {
    const traceIndex = getNextTraceIndex();
    // might not have had a trace that visited this print node
    if (traceIndex >= 0) {
      appData.traceIndex = traceIndex;
    }
    appData.selectedNodeId = printNodeId;
    refreshApp();

    function getNextTraceIndex() {
      if (appData.selectedNodeId === printNodeId) {
        const traceIndex = traceResult.traces.findIndex((t, index) => index > appData.traceIndex && t.printNodeId === printNodeId);
        if (traceIndex >= 0) {
          return traceIndex;
        }
      }

      return traceResult.traces.findIndex(t => t.printNodeId === printNodeId);
    }
  });

  const mainElement = document.createElement("div");
  mainElement.id = "main";
  const splitViewElement = document.createElement("div");
  splitViewElement.id = "split-view";
  splitViewElement.appendChild(codeView.element);
  const graphContainer = document.createElement("div");
  graphContainer.id = "graph-container";
  graphContainer.appendChild(graph.element);
  const nodeInfoArea = createNodeInfoArea();
  graphContainer.appendChild(nodeInfoArea.element);
  splitViewElement.appendChild(graphContainer);
  mainElement.appendChild(splitViewElement);
  mainElement.appendChild(infoArea.element);
  mainElement.appendChild(slider.element);
  document.body.appendChild(mainElement);

  refreshApp();

  function refreshApp() {
    slider.setMax(traceResult.traces.length - 1);
    slider.setValue(appData.traceIndex);
    codeView.setTraceIndex(appData.traceIndex);
    infoArea.setTraceIndex(appData.traceIndex);
    graph.setSelectedNodeId(appData.selectedNodeId);
    nodeInfoArea.setSelectedNodeId(appData.selectedNodeId);
  }
}

/** @param {import("./types").PrintNode} node */
function getLastNodes(node) {
  while (node.nextPrintNodeId != null) {
    node = traceResult.getPrintNode(node.nextPrintNodeId);
  }

  /** @type {import("./types").PrintNode[]} */
  const lastNodes = [];
  if (node.printItem.kind === "condition") {
    const condition = node.printItem.content;
    if (condition.truePath == null || condition.falsePath == null) {
      lastNodes.push(node);
    }
    if (condition.truePath != null) {
      lastNodes.push(...getLastNodes(traceResult.getPrintNode(condition.truePath)));
    }
    if (condition.falsePath != null) {
      lastNodes.push(...getLastNodes(traceResult.getPrintNode(condition.falsePath)));
    }
  } else if (node.printItem.kind === "rcPath") {
    lastNodes.push(...getLastNodes(traceResult.getPrintNode(node.printItem.content)));
  } else {
    lastNodes.push(node);
  }
  return lastNodes;
}

/** @param {import("./types").PrintNode} node */
function getNodeHoverText(node) {
  const printItem = node.printItem;
  switch (printItem.kind) {
    case "condition":
      return `Condition: ${printItem.content.name} (${node.printNodeId})`;
    case "info":
      return `Info: ${printItem.content.content.name} - ${printItem.content.kind} (${node.printNodeId})`;
    case "rcPath":
      return `RcPath (${node.printNodeId})`;
    case "signal":
      return `Signal: ${printItem.content} (${node.printNodeId})`;
    case "string":
      return `String: ${printItem.content} (${node.printNodeId})`;
    case "anchor":
      return `Anchor: ${printItem.content.name} (${node.printNodeId})`;
    case "conditionReevaluation":
      return `Condition reevaluation: ${printItem.content.name} (${printItem.content.conditionId}) (${node.printNodeId})`;
  }
}

function getNodesAndLinks() {
  /** @type {import("./types").GraphPrintNode[]} */
  const nodes = traceResult.printNodes.map(node => ({ id: node.printNodeId, printNode: node, sources: [], targets: [], depthY: 0 }));
  const nodesMap = new Map(nodes.map(n => [n.printNode.printNodeId, n]));
  /** @type {{ source: number; target: number; color: string | undefined; originatingNodeId: number | undefined }[]} */
  const links = [];

  for (const node of nodes) {
    const printItem = node.printNode.printItem;
    const printNode = node.printNode;
    if (printItem.kind === "rcPath") {
      const target = getNodeById(printItem.content);
      addLink(node, target);
      addLinksToLastNodes(node, target);
    } else if (printItem.kind === "condition") {
      const condition = printItem.content;
      if (condition.truePath != null) {
        const target = getNodeById(condition.truePath);
        addLink(node, target, "green");
        addLinksToLastNodes(node, target);
      }
      if (condition.falsePath != null) {
        const target = getNodeById(condition.falsePath);
        addLink(node, target, "red");
        addLinksToLastNodes(node, target);
      }
      if ((condition.truePath == null || condition.falsePath == null) && printNode.nextPrintNodeId != null) {
        addLink(node, getNodeById(printNode.nextPrintNodeId));
      }
    } else if (printNode.nextPrintNodeId != null) {
      addLink(node, getNodeById(printNode.nextPrintNodeId));
    }
  }

  setDepthY(nodes[0]);

  return { nodes, links };

  /** @param {import("./types").GraphPrintNode} firstNode */
  function setDepthY(firstNode) {
    if (firstNode.sources.length > 0) {
      throw new Error("Must provide the root node.");
    }

    /** @type {Set<number>} */
    const analyzedNodes = new Set();
    const nodesToAnalyze = [firstNode];

    while (nodesToAnalyze.length > 0) {
      const node = nodesToAnalyze.pop();
      if (node == null) {
        continue;
      }
      node.depthY = node.sources.length === 0 ? 0 : Math.max(...node.sources.map(s => s.depthY)) + 1;
      if (!analyzedNodes.has(node.printNode.printNodeId)) {
        analyzedNodes.add(node.printNode.printNodeId);
        nodesToAnalyze.push(...node.targets);
      }
    }
  }

  /**
   * @param {import("./types").GraphPrintNode} source
   * @param {import("./types").GraphPrintNode} target
   */
  function addLinksToLastNodes(source, target) {
    if (source.printNode.nextPrintNodeId != null) {
      const nextPrintNodeTarget = getNodeById(source.printNode.nextPrintNodeId);
      for (const lastNode of getLastNodes(target.printNode)) {
        addLink(getNodeById(lastNode.printNodeId), nextPrintNodeTarget, undefined, source.printNode.printNodeId);
      }
    }
  }

  /**
   * @param {import("./types").GraphPrintNode} source
   * @param {import("./types").GraphPrintNode} target
   * @param {string} [color]
   * @param {number} [originatingNodeId]
   */
  function addLink(source, target, color, originatingNodeId) {
    if (color == null && source.printNode.printItem.kind === "condition") {
      const condition = source.printNode.printItem.content;
      color = condition.truePath == null && condition.falsePath == null
        ? undefined
        : condition.falsePath == null
        ? "red"
        : "green";
    }
    source.targets.push(target);
    target.sources.push(source);
    links.push({
      source: source.id,
      target: target.id,
      color,
      originatingNodeId,
    });
  }

  /** @param {number} id */
  function getNodeById(id) {
    const node = nodesMap.get(id);
    if (node == null) {
      throw new Error(`Could not find node: ${id}`);
    }
    return node;
  }
}

/** @param {(printNodeId: number) => void} onPrintNodeSelect */
function createGraph(onPrintNodeSelect) {
  const { links, nodes } = getNodesAndLinks();

  let wasMouseActivity = false;
  const width = 400;
  const height = 400;
  const simulation = d3.forceSimulation(nodes)
    .force("link", d3.forceLink(links).id(/** @param {any} d */ d => d.id).distance(10))
    .force("charge", d3.forceManyBody().strength(-3000))
    .force("y", d3.forceY().y(/** @param {any} d */ d => d.depthY * 125));
  const svg = d3.create("svg")
    .attr("viewBox", [0, 0, width, height])
    .style("font", "40px sans-serif")
    .on("wheel", () => wasMouseActivity = true)
    .on("click", () => wasMouseActivity = true);

  const arrow = svg.append("svg:defs").selectAll("marker")
    .data(["end"])
    .enter().append("svg:marker")
    .attr("id", String)
    .attr("orient", "auto");
  const arrowInnerPath = arrow.append("svg:path").attr("fill", "#000");

  const drag = d3
    .drag()
    .on("drag", /** @param {any} event @param {any} d */ function(event, d) {
      d.x = event.x;
      d.y = event.y;
      d3.select(this).raise().attr("transform", `translate(${d.x}, ${d.y})`);
      refreshLinks();
    });

  const nodeRadius = 15;
  const linkThickness = 5;
  const linkG = svg.append("g");
  const link = linkG
    .selectAll("line")
    .data(links)
    .join("line")
    .attr("stroke-opacity", 0.6)
    .attr("stroke", /** @param {any} d */ d => getLineColor(d))
    .attr("data-originating-node-id", /** @param {any} d */ d => d.originatingNodeId)
    .style("stroke-width", linkThickness)
    .attr("marker-end", "url(#end)")
    .on("click", /** @param {any} _ @param {any} d */ (_, d) => {
      /** @type {number | undefined} */
      const originatingNodeId = d.originatingNodeId;
      if (originatingNodeId != null) {
        onPrintNodeSelect(originatingNodeId);
      }
    });
  link.append("title")
    .text(/** @param {any} d */ d => {
      if (d.originatingNodeId != null) {
        return getNodeHoverText(traceResult.getPrintNode(d.originatingNodeId));
      }
      return undefined;
    });

  const nodeG = svg.append("g");
  const nodeGInner = nodeG.append("g")
    .selectAll("g")
    .data(nodes)
    .join("g")
    .call(drag);
  const nodeCircle = nodeGInner
    .append("circle")
    .attr("r", nodeRadius)
    .attr("fill", /** @param {any} d */ d => getNodeColor(d.printNode))
    .attr("stroke", "#000")
    .attr("id", /** @param {any} d */ d => `node${d.id}`)
    .on("click", /** @param {any} _ @param {any} d */ (_, d) => {
      onPrintNodeSelect(d.id);
    });
  nodeGInner
    .append("text")
    .attr("x", 50)
    .attr("y", "0.31em")
    .text(/** @param {any} d */ d => getNodeHoverText(d.printNode))
    .clone(true).lower()
    .attr("fill", "none")
    .attr("stroke", "white")
    .attr("stroke-width", 3);

  /** @type {any} */
  let transform;
  /** @type {number} */
  let sqrtK;
  const zoom = d3.zoom().on("zoom", /** @param {any} e */ e => {
    transform = e.transform;
    nodeG.attr("transform", transform);
    sqrtK = Math.sqrt(transform.k);
    nodeCircle.attr("r", nodeRadius / sqrtK)
      .attr("stroke-width", 1 / sqrtK);

    linkG.attr("transform", transform);
    link.style("stroke-width", linkThickness / sqrtK);

    arrow.attr("markerWidth", 5)
      .attr("markerHeight", 5)
      .attr("viewBox", `0 0 ${5 / sqrtK} ${5 / sqrtK}`)
      .attr("refX", 8 / sqrtK)
      .attr("refY", 2.5 / sqrtK);
    arrowInnerPath.attr("d", `M 0 0 L ${5 / sqrtK} ${2.5 / sqrtK} L 0 ${5 / sqrtK} z`);
    refreshSelectedNode();
  });

  simulation.on("tick", () => {
    refreshLinks();

    let minX = Number.MAX_SAFE_INTEGER;
    let maxX = Number.MIN_SAFE_INTEGER;
    let minY = Number.MAX_SAFE_INTEGER;
    let maxY = Number.MIN_SAFE_INTEGER;
    nodeGInner
      .attr("transform", /** @param {any} d */ d => {
        minX = Math.min(minX, d.x);
        maxX = Math.max(maxX, d.x);
        minY = Math.min(minY, d.y);
        maxY = Math.max(maxY, d.y);
        return `translate(${d.x}, ${d.y})`;
      });

    if (!wasMouseActivity) {
      svg.call(
        zoom.transform,
        d3.zoomIdentity
          .translate(width / 2, height / 2)
          .scale(0.95 / Math.max((maxX - minX) / width, (maxY - minY) / height))
          .translate(-(minX + maxX) / 2, -(minY + maxY) / 2),
      );
    }
  });

  function refreshLinks() {
    link
      .attr("x1", /** @param {any} d */ d => d.source.x)
      .attr("y1", /** @param {any} d */ d => d.source.y)
      .attr("x2", /** @param {any} d */ d => d.target.x)
      .attr("y2", /** @param {any} d */ d => d.target.y);
  }

  let lastId = 0;
  return {
    element: svg
      .call(zoom)
      .call(zoom.transform, d3.zoomIdentity)
      .node(),
    /** @param {number} selectedNodeId */
    setSelectedNodeId(selectedNodeId) {
      d3.select(`#node${lastId}`)
        .attr("stroke", "#000")
        .attr("stroke-width", 1 / sqrtK)
        .attr("r", nodeRadius / sqrtK);
      d3.selectAll(`[data-originating-node-id="${lastId}"]`)
        .style("stroke-width", linkThickness / sqrtK)
        .attr("marker-end", "url(#end)");
      lastId = selectedNodeId;
      refreshSelectedNode();
    },
  };

  function refreshSelectedNode() {
    d3.select(`#node${lastId}`)
      .attr("stroke", "red")
      .attr("stroke-width", 4 / sqrtK)
      .attr("r", (nodeRadius + 10) / sqrtK);
    d3.selectAll(`[data-originating-node-id="${lastId}"]`)
      .style("stroke-width", (linkThickness + 15) / sqrtK)
      // not worth the hassle to resize this
      .attr("marker-end", "");
  }

  /** @param {any} d */
  function getLineColor(d) {
    return d.color || (d.originatingNodeId != null ? "blue" : "#000");
  }
}

function createNodeInfoArea() {
  const containerElement = document.createElement("div");
  containerElement.id = "node-info-area";
  const colorRectangle = document.createElement("span");
  colorRectangle.id = "color-rectangle";
  containerElement.appendChild(colorRectangle);
  const nameElement = document.createElement("span");
  containerElement.appendChild(nameElement);

  return {
    element: containerElement,
    /** @param {number} selectedNodeId */
    setSelectedNodeId(selectedNodeId) {
      const printNode = traceResult.getPrintNode(selectedNodeId);
      colorRectangle.style.backgroundColor = getNodeColor(printNode);
      nameElement.textContent = getNodeHoverText(printNode);
    },
  };
}

function createInfoArea() {
  const mainElement = document.createElement("div");
  mainElement.id = "info-area";
  const currentTimeLabel = document.createElement("label");
  currentTimeLabel.textContent = "Time:";
  mainElement.appendChild(currentTimeLabel);
  const timeSpan = document.createElement("span");
  mainElement.appendChild(timeSpan);

  return {
    element: mainElement,
    /** @param {number} index */
    setTraceIndex(index) {
      const trace = traceResult.traces[index];
      timeSpan.textContent = formatNanos(trace.nanos);
    },
  };
}

function createCodeView() {
  const tabChars = "→&nbsp;&nbsp;&nbsp;";
  const spaceChar = "·";
  const mainElement = document.createElement("div");
  mainElement.id = "code-view";
  let lastTraceIndex = -1;

  return {
    element: mainElement,
    /** @param {number} index */
    setTraceIndex(index) {
      if (lastTraceIndex === index) {
        return;
      }

      const trace = traceResult.traces[index];
      clearElementChildren(mainElement);

      if (trace.writerNodeId != null) {
        const startWriterNode = traceResult.getWriterNode(trace.writerNodeId);

        /** @type {HTMLElement[]} */
        const elements = [];
        fillWriterNodeElements(startWriterNode, elements);
        elements.reverse(); // reverse to get them in forward order

        for (const childElement of elements) {
          mainElement.appendChild(childElement);
        }
      }

      // scroll to the bottom
      mainElement.scrollTop = mainElement.scrollHeight;

      lastTraceIndex = index;
    },
  };

  /**
   * @param {import("./types").WriterNode} node
   * @param {HTMLElement[]} elements
   */
  function fillWriterNodeElements(node, elements) {
    if (node.text === "\n" || node.text === "\r\n") {
      elements.push(document.createElement("br"));
    } else {
      const texts = node.text.split(/\r?\n/);
      for (const [i, text] of texts.entries()) {
        if (i > 0) {
          elements.push(document.createElement("br"));
        }

        for (const segment of extractTextSegments(text).reverse()) {
          if (segment.kind === "text") {
            const element = document.createElement("span");
            element.className = "writer-node";
            element.innerText = segment.text;
            elements.push(element);
          } else if (segment.kind === "space") {
            const element = document.createElement("span");
            element.className = "writer-node writer-node-space";
            element.innerText = spaceChar.repeat(segment.count);
            elements.push(element);
          } else if (segment.kind === "tab") {
            const element = document.createElement("span");
            element.className = "writer-node writer-node-tab";
            element.innerHTML = tabChars.repeat(segment.count);
            elements.push(element);
          }
        }
      }
    }

    if (node.previousNodeId != null) {
      fillWriterNodeElements(
        traceResult.getWriterNode(node.previousNodeId),
        elements,
      );
    }
  }
}

/** @param {HTMLElement} element */
function clearElementChildren(element) {
  let last;
  while (last = element.lastChild) {
    element.removeChild(last);
  }
}

/** @param {(value: number) => void} onChange */
function createSlider(onChange) {
  const element = document.createElement("div");
  element.id = "slider";
  const input = document.createElement("input");
  input.type = "range";
  input.addEventListener("input", () => {
    // todo: debounce
    onChange(input.valueAsNumber);
  });
  input.min = "0";

  element.appendChild(input);

  return {
    element,
    /** @param {number} max */
    setMax(max) {
      if (input.max !== max.toString()) {
        input.max = max.toString();
      }
    },
    /** @param {number} value */
    setValue(value) {
      if (input.value !== value.toString()) {
        input.value = value.toString();
      }
    },
  };
}

function getTransformedTraceResult() {
  const writerNodes = getWriterNodesMap();
  const printNodes = getPrintNodesMap();
  return {
    traces: rawTraceResult.traces,
    printNodes: rawTraceResult.printNodes,
    /** @param {number} id */
    getWriterNode(id) {
      const node = writerNodes.get(id);
      if (node == null) {
        throw new Error(`Could not find writer node ${id}.`);
      }
      return node;
    },
    /** @param {number} id */
    getPrintNode(id) {
      const node = printNodes.get(id);
      if (node == null) {
        throw new Error(`Could not find print node ${id}.`);
      }
      return node;
    },
  };

  function getWriterNodesMap() {
    /** @type {Map<number, import("./types").WriterNode>} */
    const map = new Map();
    for (const node of rawTraceResult.writerNodes) {
      map.set(node.writerNodeId, node);
    }
    return map;
  }

  function getPrintNodesMap() {
    /** @type {Map<number, import("./types").PrintNode>} */
    const map = new Map();
    for (const node of rawTraceResult.printNodes) {
      map.set(node.printNodeId, node);
    }
    return map;
  }
}

/** @param {import("./types").PrintNode} node */
function getNodeColor(node) {
  switch (node.printItem.kind) {
    case "info":
      return "blue";
    case "condition":
      return "orange";
    case "signal":
      return "yellow";
    case "rcPath":
      return "green";
    case "string":
      return "#ccc";
    case "anchor":
      return "pink";
    case "conditionReevaluation":
      return "purple";
  }
}

/** @param {number} nanos */
function formatNanos(nanos) {
  const characters = nanos.toString();
  let finalText = "";
  for (let i = 0; i < characters.length; i++) {
    if (i > 0 && i % 3 === 0) {
      finalText = "," + finalText;
    }
    finalText = characters[characters.length - 1 - i] + finalText;
  }
  return finalText + "ns";
}

/** @param {string} text */
function extractTextSegments(text) {
  let spaceCount = 0;
  let tabCount = 0;
  let lastIndex = 0;
  /** @type {import("./types").CodeViewTextSegment[]} */
  const segments = [];

  for (let i = 0; i < text.length; i++) {
    const char = text[i];
    if (char === " ") {
      addTabIfNecessary();
      addLastTextIfNecessary(i);
      spaceCount++;
      lastIndex = i + 1;
    } else if (char === "\t") {
      addSpaceIfNecessary();
      addLastTextIfNecessary(i);
      tabCount++;
      lastIndex = i + 1;
    } else {
      addSpaceIfNecessary();
      addTabIfNecessary();
    }
  }

  addLastTextIfNecessary(text.length);
  addSpaceIfNecessary();
  addTabIfNecessary();

  return segments;

  function addTabIfNecessary() {
    if (tabCount > 0) {
      segments.push({
        kind: "tab",
        count: tabCount,
      });
      tabCount = 0;
    }
  }

  function addSpaceIfNecessary() {
    if (spaceCount > 0) {
      segments.push({
        kind: "space",
        count: spaceCount,
      });
      spaceCount = 0;
    }
  }

  /** @param {number} currentIndex */
  function addLastTextIfNecessary(currentIndex) {
    if (lastIndex !== currentIndex) {
      segments.push({
        kind: "text",
        text: text.substring(lastIndex, currentIndex),
      });
    }
  }
}
