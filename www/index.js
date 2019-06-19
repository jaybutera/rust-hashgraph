import * as wasm from "rust-hashgraph";

// Globals
let creators = {}
  , nodes = []
  , links = [];


let g = new_graph('yo_id');
console.log("Adding an event to the graph: " + add_event(null, 'yo_id'));//g.add(null, []));
console.log("Adding this event should fail: " + !g.add("test", []));

//const json_events = Object.entries(JSON.parse(g.get_graph()).events);

function new_graph(creator_id) {
    creators[creator_id] = wasm.Graph.new(creator_id);
    return creators[creator_id];
}

function add_event(other_parent, creator_id) {
    let g = creators[creator_id];
    return g.add(other_parent, []); // Returns new event hash (or null on failure)
}

document.getElementById('addEvent').addEventListener('click', () => add_event(null, 'yo_id'));

function add_event(other_parent, creator_id) {
    let g = creators[creator_id];
    //let op = null;
    //let h = add_event(op, 'yo_id');
    let e = Object.values(JSON.parse( g.get_event(h) ))[0];
    console.log("event: " + g.get_event(h));
    console.log(e.self_parent)

    // Update d3
    if (h) {
        nodes.push( Object.create({
            creator: 'yo_id',
            hash: h
        }));
        links.push({
            source: e.self_parent,
            target: h
        });
        if (op) {
            links.push({
                source: op,
                target: h
            });
        }
    }
    node.data(nodes).enter().merge();
    link.data(links).enter().merge();

    console.log("NODES");
    console.log(nodes);
    console.log("LINKS");
    console.log(links);
});

/*
let nodes = json_events.map(e => Object.create({
    creator: Object.values(e[1])[0].creator,
    hash: e[0]
}));

let links = flatten(json_events.map(e => {
    const hash = e[0];
    const info = Object.values(e[1])[0]; // Get event regardless of Genesis or Update

    if ("self_parent" in info) {
        let links = [{
            source: info.self_parent,
            target: hash,
        }];

        if ("other_parent" in info && info.other_parent != null)
            links.push( {
                source: info.other_parent,
                target: hash,
            });

        return links;
    }

    return null;
}).filter(x => x != null));

console.log("NODES");
console.log(nodes);
console.log("LINKS");
console.log(links);

function flatten(arr) {
    return [].concat(...arr)
}
*/

let drag = simulation => {
  function dragstarted(d) {
    if (!d3.event.active) simulation.alphaTarget(0.3).restart();
    d.fx = d.x;
    d.fy = d.y;
  }

  function dragged(d) {
    d.fx = d3.event.x;
    d.fy = d3.event.y;
  }

  function dragended(d) {
    if (!d3.event.active) simulation.alphaTarget(0);
    d.fx = null;
    d.fy = null;
  }

  return d3.drag()
      .on("start", dragstarted)
      .on("drag", dragged)
      .on("end", dragended);
}

const svg = d3.select("body")
    .append('svg')
    .attr('width', 500)
    .attr('height', 200);

const width = svg.attr('width'),
      height = svg.attr('height');

const sim = d3.forceSimulation()
    .nodes(nodes)
    .force('link', d3.forceLink(links).id(n => n.hash))
    .force('charge', d3.forceManyBody())
    .force('center', d3.forceCenter(width/2, height/2));

const link = svg.append("g")
    .attr("stroke", "#999")
    .attr("stroke-opacity", 0.6)
    .selectAll("line")
    .data(links)
    .join("line")
    .attr("stroke-width", d => Math.sqrt(d.value));

const node = svg.append("g")
    .attr("stroke", "#fff")
    .attr("stroke-width", 1.5)
    .selectAll("circle")
    .data(nodes)
    .join("circle")
    .attr("r", 5)
    .attr("fill", d => {
      const scale = d3.scaleOrdinal(d3.schemeCategory10);
      return d => scale(d.group);
    })
    .call(drag(sim));

node.append("title").text(d => d.creator);

sim.on("tick", () => {
    link
        .attr("x1", d => d.source.x)
        .attr("y1", d => d.source.y)
        .attr("x2", d => d.target.x)
        .attr("y2", d => d.target.y);

    node
        .attr("cx", d => d.x)
        .attr("cy", d => d.y);
});
