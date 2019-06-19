import * as wasm from "rust-hashgraph";

// Globals
let creators = {}
  , nodes = []
  , links = [];

document.getElementById('addEvent').addEventListener('click', () => {
    let other_parent = document.getElementById('inp_hash').value;
    console.log(other_parent);

    if (!other_parent) other_parent = null;
    add_event(other_parent, 'yo_id')
});

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
      height = svg.attr('height'),
      color = d3.scaleOrdinal(d3.schemeCategory10);

const sim = d3.forceSimulation()
    .nodes(nodes)
    .force('link', d3.forceLink(links).id(n => n.hash))
    .force('charge', d3.forceManyBody())
    .force('center', d3.forceCenter(width/2, height/2));

let link = svg.append("g")
    .attr("stroke", "#999")
    .attr("stroke-opacity", 0.6)
    .selectAll("line")
    .data(links)
    .join("line")
    .attr("stroke-width", d => Math.sqrt(d.value));

let node = svg.append("g")
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

function new_graph(creator_id) {
    let g = wasm.Graph.new(creator_id);
    // Store in creators dict
    creators[creator_id] = g;

    const genesis_hash = Object.entries(JSON.parse(g.get_graph()).events)[0][0];
    nodes.push( Object.create({
        creator: 'yo_id',
        hash: genesis_hash
    }));

    restart();
    display_hash(genesis_hash);
    return creators[creator_id];
}

function add_event(other_parent, creator_id) {
    let g = creators[creator_id];
    let h = g.add(other_parent, []);
    let self_parent = Object.values(JSON.parse( g.get_event(h) ))[0].self_parent;

    // Update d3
    if (h) {
        nodes.push( Object.create({
            creator: 'yo_id',
            hash: h
        }));
        links.push({
            source: self_parent,
            target: h
        });
        if (other_parent) {
            links.push({
                source: other_parent,
                target: h
            });
        }

        restart();
        display_hash(h);
    }


    console.log("NODES");
    console.log(nodes);
    console.log("LINKS");
    console.log(links);

    return h;
};

function display_hash(hash) {
    let p = document.createElement('p');
    p.innerText = hash;
    document.getElementById('hashes')
            .appendChild(p);
}

function restart() {
  // Apply the general update pattern to the nodes.
  node = node.data(nodes, function(d) { return d.hash;});
  node.exit().remove();
  node = node.enter().append("circle").attr("fill", function(d) { return color(d.hash); }).attr("r", 8).merge(node);

  // Apply the general update pattern to the links.
  link = link.data(links, function(d) { return d.source.hash + "-" + d.target.id; });
  link.exit().remove();
  link = link.enter().append("line").merge(link);

  // Update and restart the simulation.
  sim.nodes(nodes);
  sim.force("link").links(links);
  sim.alpha(1).restart();
}

// Start a graph
let creator = 'yo_id'
let g = new_graph(creator);
