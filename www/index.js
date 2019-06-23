import * as wasm from "rust-hashgraph";

// Globals
let creators = {}
  , nodes = []
  , links = []
  // Creator id -> event hash
  , latest_event = {};

/*
document.getElementById('addEvent').addEventListener('click', () => {
    let other_parent = document.getElementById('inp_hash').value;
    let creator_id = document.getElementById('creator_id').value;

    if (!other_parent) other_parent = null;
    if (creator_id)
        add_event(other_parent, creator_id)
});
*/

document.getElementById('addCreator').addEventListener('click', () => {
    let creator_id = document.getElementById('creator_id').value;
    if (creator_id) { new_creator(creator_id); }
});

document.getElementById('createEvent').addEventListener('click', () => {
    const by_id   = document.getElementById('by_creator').value;
    const from_id = document.getElementById('from_creator').value;

    let other_parent;
    if (!from_id)
        other_parent = null;
    else
        other_parent = latest_event[from_id];

    if (by_id)
        add_event(other_parent, by_id)
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
    .attr('width', 800)
    .attr('height', 300);

const width = svg.attr('width'),
      height = svg.attr('height'),
      color = d3.scaleOrdinal(d3.schemeCategory10);

const sim = d3.forceSimulation()
    .nodes(nodes)
    .force('link', d3.forceLink(links).id(n => n.hash).distance(30))
    .force('charge', d3.forceManyBody())
    .force('center', d3.forceCenter(width/2, height/2));

let link = svg.append("g")
    .attr("stroke", "#999")
    .attr("stroke-opacity", 0.6)
    .selectAll("line")
    .data(links)
    .join("line")
    .attr("stroke-width", d => Math.sqrt(d.value));

let nodeGroup = svg.append('g')
    .classed('node', true);

sim.on("tick", () => {
    link
        .attr("x1", d => d.source.x)
        .attr("y1", d => d.source.y)
        .attr("x2", d => d.target.x)
        .attr("y2", d => d.target.y);

    let n = nodeGroup.selectAll('g');
    n.select('circle')
        .attr("cx", d => d.x)
        .attr("cy", d => d.y);
    n.select('text')
        .attr("x", d => d.x)
        .attr("y", d => d.y);
});

function new_graph(creator_id) {
    let g = wasm.Graph.new(creator_id);

    const genesis_hash = Object.entries(JSON.parse(g.get_graph()).events)[0][0];
    nodes.push( Object.create({
        creator: creator_id,
        hash: genesis_hash
    }));

    restart();

    // Update latest event global record
    latest_event[creator_id] = genesis_hash;

    //display_hash(genesis_hash, g);
    return g;
}

function new_creator(creator_id) {
    let h = g.add_creator(creator_id);

    if (h) {
        nodes.push( Object.create({
            creator: creator_id,
            hash: h
        }));

        restart();

        // Update latest event global record
        latest_event[creator_id] = h;

        //display_hash(h,g);
    }
}

function add_event(other_parent, creator_id) {
    let h = g.add(other_parent, creator_id, []);
    let self_parent = Object.values(JSON.parse( g.get_event(h) ))[0].self_parent;

    // Update d3
    if (h) {
        nodes.push( Object.create({
            creator: creator_id,
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

        // Update latest event global record
        latest_event[creator_id] = h;

        //display_hash(h,g);
    }


    console.log("NODES");
    console.log(nodes);
    console.log("LINKS");
    console.log(links);

    return h;
};

function display_hash(hash, graph) {
    let p = document.createElement('p');
    p.innerText = hash + ": round " + graph.round_of(hash);
    document.getElementById('hashes')
            .appendChild(p);
}

function restart() {
    // Add new circle/labels
    let n = nodeGroup.selectAll('g').data(nodes).enter().append('g');//.on('mouseover', console.log('hey'));
    n
        .append('circle')
        .attr('fill', d => { return color(d.creator) })
        .attr('r', 8).call(drag(sim));
    //n
        //.append('text')
        //.text(d => { return d.hash });

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
let creator = 'c1'
let g = new_graph(creator);
