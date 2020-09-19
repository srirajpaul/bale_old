use chrono::{DateTime, Local};
use spmat::wall_seconds;
use spmat::SparseMat;
use std::fs::OpenOptions;
use std::io::Write;
use std::io::Error;
use std::ops::Range;
use std::path::Path;

// A helper function for dumping only part of a big data structure.
// This should really go somewhere else than the delta_stepper lib.
pub fn display_ranges(max_disp: usize, num_items: usize) -> Vec<Range<usize>> {
        let mut ranges: Vec<Range<usize>> = Vec::new();
        if max_disp <= num_items && max_disp > 0 {
            ranges.push(0..max_disp/2);
            ranges.push(num_items-max_disp/2..num_items);
        } else {
            ranges.push(0..num_items);
        }
        ranges
}

// Output structure for single-source shortest path
#[derive(Debug, Clone)]
pub struct SsspInfo {
    pub distance: Vec<f64>,
    pub source: usize,
    pub laptime: f64,
}

impl SsspInfo {
    // Dump output distances to a file
    pub fn dump(&self, max_disp: usize, filename: &str) -> Result<(),Error> { 
        let path = Path::new(&filename);
        let mut file = OpenOptions::new().write(true).create(true).open(path)?;
        writeln!(file, "==========================================================")?;
        let now: DateTime<Local> = Local::now();
        writeln!(file, "Final Distances at {}", now)?;

        write!(file, "vtx: dist\n")?;
        for r in display_ranges(max_disp, self.distance.len()) {
            for v in r {  
                write!(file, "{}: {}\n", v, self.distance[v])?;
            }
        }
        Ok(())
    }

    // Dump output distances to a file in Phil's .wts format
    // needs parallel version 0-0
    pub fn dump_wts(&self, filename: &str) -> Result<(),Error> { 
        let path = Path::new(&filename);
        let mut file = OpenOptions::new().write(true).create(true).open(path)?;
        writeln!(file, "{}", self.distance.len())?;
        for v in &self.distance {
            writeln!(file, "{}", v)?;
        }
        Ok(())
    }

}

// A potential edge relaxation to be examined
struct Request {
    w_g: usize,  // head of edge being relaxed, global vertex index
    dist: f64,   // new distance from source to w_g using that edge
//  v_g: usize,  // could include tail of edge (v_g,w_g) in request to build shortest path tree
}

// A struct and methods for all the data structures in delta stepping
// Each bucket is a circular doubly-linked list, linked by prev_elt and next_elt,
// indexed by an "elt" that is either a vertex in the bucket, or a "bucket header".
// A vertex's "elt" number is its vertex number.
// The header of bucket i (with reuse) is "elt" number num_buckets + i.

struct BucketSearcher<'a> {
    graph: &'a SparseMat,           // the graph being searched
    delta: f64,                     // width of a bucket
    num_buckets: usize,             // number of actual buckets, taking into account reuse
    tentative_dist: Vec<f64>,       // current tentative distance from source to this vtx
    prev_elt: Vec<usize>,           // back link in list of vertices in each bucket (including bucket header)
    next_elt: Vec<usize>,           // forward link in list of vertices in each bucket (including header)
    activated: Vec<bool>,           // has this vtx ever been activated? 
    vtx_bucket: Vec<Option<usize>>, // what bucket if any is this vtx in?
    bucket_size: Vec<usize>,        // number of vertices (from this rank) in this bucket
    bucket_header: Vec<usize>,      // which elt is this bucket's header? (just to make code clearer)
//  perhaps should copy convey out of the graph?

    // parallel notes: 
    // Vertices belong to PEs in round-robin order. 
    // Each PE has its own copy of every bucket, but only containing vertices it owns.
    // (Say nv >= 100 * num_buckets * THREADS. Then storage for buckets is relatively small.)
    // Arrays bucket_size and bucket_header are local to each PE.
    // Arrays indexed only by vtx are split among PEs (but don't have to be shared, I think): 
    //     tentative_dist, activated, bucket.
    // Arrays prev_elt and next_elt are also split, linking together only the vtxs on the local PE.
    // The bucket-header nodes at the end of prev_elt and next_elt have copies on each PE.
    // The parallelism all happens in relax_requests, where a request to relax an edge with head w_g
    // gets conveyed to the PE that owns w_g.
}

impl<'a> BucketSearcher<'a> { 

    // Create a bucket structure for a weighted graph
    fn new(graph: &SparseMat, delta: f64) -> BucketSearcher { 
        let nvtxs = graph.numrows;
        let nvtxs_this_rank = graph.numrows_this_rank;

        let mut max_edge_len: f64 = 0.0;
        if let Some(edge_len) = &graph.value { 
            for c in edge_len {
                if *c > max_edge_len { 
                    max_edge_len = *c;
                }
            }
        } else {
            panic!("Graph must have edge weights (values)");
        }
        let max_edge_len = graph.convey.reduce_max(max_edge_len);
        println!("max_edge_len = {}", max_edge_len);
        // upper bound on number of buckets we will ever need at the same time
        let num_buckets = (max_edge_len/delta).ceil() as usize + 1;
        // tentative distances all start out infinite, including source
        let tentative_dist = vec![f64::INFINITY; nvtxs_this_rank];    
        // circular linked lists have room for the bucket headers,
        // and initially every list is empty (every element points to itself).
        let prev_elt: Vec<usize> = (0..nvtxs_this_rank+num_buckets).collect();
        let next_elt: Vec<usize> = (0..nvtxs_this_rank+num_buckets).collect();
        // initially no vtx has ever been activated.
        let activated = vec![false; nvtxs_this_rank];
        // initially every vtx is in no bucket.
        let vtx_bucket = vec![Option::<usize>::None; nvtxs_this_rank];
        // initially every bucket contains no vertices.
        let bucket_size = vec![0; num_buckets];
        // immutable bucket_header is just for clearer code
        let bucket_header: Vec<usize> = (nvtxs_this_rank..nvtxs_this_rank+num_buckets).collect();
        
        graph.barrier();

        BucketSearcher {
            graph,
            delta,
            num_buckets,
            tentative_dist,
            prev_elt,
            next_elt,
            activated,
            vtx_bucket,
            bucket_size,
            bucket_header,
        }
    }

    // Dump bucket structure state to a file
    fn dump(&self, max_disp: usize, filename: &str, title: &str, nums: Vec<usize>) -> Result<(),Error> { 
        let path = Path::new(&filename);
        let mut file = OpenOptions::new().append(true).create(true).open(path)?;
        let nvtxs_this_rank = self.graph.numrows_this_rank;
        let now: DateTime<Local> = Local::now();
        writeln!(file, "==========================================================")?;
        write!(file, "BucketSearcher: rank {}: {}", self.my_rank(), title)?;
        for n in nums {
            write!(file, " {}", n)?;
        }
        writeln!(file, " at {}\n", now)?;
        writeln!(file, 
            "nvtxs_this_rank={}, num_buckets={}, delta={}", 
             nvtxs_this_rank, self.num_buckets, self.delta
        )?;
        writeln!(file, "elt: global_vtx prev_elt next_elt vtx_bucket activated tentative_dist")?;
        for r in display_ranges(max_disp, nvtxs_this_rank) {
            for v in r {  
                writeln!(
                    file, 
                    "{}: {} {} {} {} {} {}", 
                    v,
                    self.global_index(v),
                    self.prev_elt[v], 
                    self.next_elt[v], 
                    if let Some(b) = self.vtx_bucket[v] {
                        b.to_string()
                    } else { 
                        "N".to_string()
                    },
                    self.activated[v], 
                    self.tentative_dist[v]
                )?;
            }
        }
        for v in nvtxs_this_rank..nvtxs_this_rank+self.num_buckets {  
            writeln!(file, "{}: {} {}", v, self.prev_elt[v], self.next_elt[v])?;
        }
        writeln!(file, "bucket (bucket_size on this rank): elt elt ...")?;
        for r in display_ranges(max_disp, self.num_buckets) {
            for b in r {  
                write!(file, "{} ({}):", b, self.bucket_size[b])?;
                let mut v = self.next_elt[self.bucket_header[b]];
                while v != self.bucket_header[b] {
                    write!(file, " {}", v)?;
                    v = self.next_elt[v];
                }
                write!(file, "\n")?;
            }
        }
        writeln!(file, " ")?;
        Ok(())
    }   

    // total size of a bucket over all ranks
    fn global_bucket_size(&self, bucket: usize) -> usize {
        let ret = self.graph.convey.reduce_sum(self.bucket_size[bucket]);
    }

    // find the next nonempty bucket after start_bucket, % num_buckets, if any
    fn next_nonempty_bucket(&self, start_bucket: usize) -> Option<usize> {
        let mut steps = 1;
        while steps < self.num_buckets 
            && self.bucket_size[(start_bucket + steps) % self.num_buckets] == 0 
        {
            steps += 1;
        }
        let steps = self.graph.convey.reduce_min(steps);
        if steps < self.num_buckets {
            Some((start_bucket + steps) % self.num_buckets)
        } else {
            None
        }
    }

    // what bucket does a vtx with this tentative distance go in?
    fn home_bucket(&self, dist: f64) -> usize {
        ((dist/self.delta).floor() as usize) % self.num_buckets
    }

    // remove a vtx from the bucket it's in. (harmless if not in a bucket). w is a local vtx index.
    fn remove_from_bucket(&mut self, w: usize) {
        if let Some(b) = self.vtx_bucket[w] {
            self.prev_elt[self.next_elt[w]] = self.prev_elt[w];
            self.next_elt[self.prev_elt[w]] = self.next_elt[w];
            self.prev_elt[w] = w;
            self.next_elt[w] = w;
            self.vtx_bucket[w] = None;
            self.bucket_size[b] -= 1;
        }
    }

    // insert a vtx into a bucket it's not in. (vtx must not be in a bucket) w is a local vtx index.
    fn place_in_bucket(&mut self, w:usize, new_bucket: usize) {
        assert!(self.vtx_bucket[w] == None);
        assert!(self.prev_elt[w] == w);
        assert!(self.next_elt[w] == w);
        self.prev_elt[w] = self.bucket_header[new_bucket];
        self.next_elt[w] = self.next_elt[self.bucket_header[new_bucket]];
        self.prev_elt[self.next_elt[w]] = w;
        self.next_elt[self.prev_elt[w]] = w;
        self.vtx_bucket[w] = Some(new_bucket);
        self.bucket_size[new_bucket] += 1;
    }

    // make a list of all relaxation requests from light edges with tails in bucket
    fn find_light_requests(&self, b: usize) -> Vec<Request> { 
        let mut requests: Vec<Request> = Vec::new();
        if let Some(edge_len) = &self.graph.value { 
            let mut v = self.next_elt[self.bucket_header[b]];
            while v != self.bucket_header[b] {
                for adj in self.graph.offset[v]..self.graph.offset[v+1] {
                    let vw_len = edge_len[adj];
                    if vw_len <= self.delta { // light edge
                        requests.push(
                            Request {
                                w_g:  self.global_index(self.graph.nonzero[adj]),
                                dist: self.tentative_dist[v] + vw_len,
                            }
                        );
                    }
                }
                v = self.next_elt[v];
            }
        } else {
            panic!("Graph must have edge weights (values)");
        }
        requests
    }

    // make a list of all relaxation requests from heavy edges with tails on vtx_list
    fn find_heavy_requests(&self, vtx_list: Vec<usize>) -> Vec<Request> { 
        let mut requests: Vec<Request> = Vec::new();
        if let Some(edge_len) = &self.graph.value { 
            for v in vtx_list {
                for adj in self.graph.offset[v]..self.graph.offset[v+1] {
                    let vw_len = edge_len[adj];
                    if vw_len > self.delta { // heavy edge
                        requests.push(
                            Request {
                                w_g:  self.graph.nonzero[adj],
                                dist: self.tentative_dist[v] + vw_len,
                            }
                        );
                    }
                }
            }
        } else {
            panic!("Graph must have edge weights (values)");
        }
        requests
    }

    // return vertices in bucket that have not been activated(removed from an active bucket) before
    fn newly_active_vertices(&self, b: usize) -> Vec<usize> { 
        let mut new_vtxs: Vec<usize> = Vec::new(); 
        let mut v = self.next_elt[self.bucket_header[b]];
        while v != self.bucket_header[b] {
            if !self.activated[v] {
                new_vtxs.push(v); 
            }
            v = self.next_elt[v];
        }
        new_vtxs
    }

    // remove all vertices from the bucket and mark them activated
    fn empty_bucket(&mut self, b: usize) { 
        let header = self.bucket_header[b];
        let mut v = self.next_elt[header];
        while v != header {
            let w = self.next_elt[v];
            self.next_elt[v] = v;
            self.prev_elt[v] = v;
            self.vtx_bucket[v] = None;
            self.activated[v] = true;
            v = w;
        }
        self.next_elt[header] = header;
        self.prev_elt[header] = header;
        self.bucket_size[b] = 0;
    }

    // relax all the requests from this phase (could be parallel)
    fn relax_requests(&mut self, requests: Vec<Request>) {
        // maybe also barrier at beginning of relax_requests? or superfluous before creating session?
        // convey the request r=(w_g,d) to the PE that owns vtx w_g here, and have it call relax
        // is there a way to do this without opening a new conveyor session at each middle loop iter?
        {
            // Always put the session in a new block, as you will
            // not be able to able to local after conveyor is done
            let mut session = self.graph.convey.begin(|item: Request, _from_rank| {
                self.relax(item);
            });
            for r in requests {
                let rank = home_rank(r.w_g);
                session.push(r, rank);
            }
            session.finish();
        }
        self.graph.convey.barrier(); // maybe this is superfluous after session.finish?
    }

    // relax an incoming edge to vtx r.w_g with new source distance r.dist, and rebucket r.w_g if necessary
    // this will be called by r.w_g's PE, so there is no race on tent[r.w_g] 
    fn relax(&mut self, r: Request) {
        let w = self.local_index(r.w_g); // local_index() panics if its argument is not on this rank
        if r.dist < self.tentative_dist[w] {
            let new_bucket = self.home_bucket(r.dist);
            if let Some(old_bucket) = self.vtx_bucket[w] {
                // w was in a bucket,
                if old_bucket != new_bucket {
                    // move w from old to new bucket
                    self.remove_from_bucket(w);
                    self.place_in_bucket(w, new_bucket);
                }
            } else {
                // w was not in a bucket, put it in new bucket
                self.place_in_bucket(w, new_bucket);
            }
            self.tentative_dist[w] = r.dist;
        }
    }
}

pub trait DeltaStepping {
    fn delta_stepping(&self, source: usize, forced_delta: Option<f64>) -> SsspInfo;
    fn check_result(&self, info: &SsspInfo, dump_files: bool) -> bool;
}

impl DeltaStepping for SparseMat {

    /// This implements the sequential AGI variant of delta stepping.
    /// # Argument: source vertex
    fn delta_stepping(&self, source: usize, forced_delta: Option<f64>) -> SsspInfo {
        assert!(self.numrows == self.numcols);
        assert!(source < self.numrows);

        let t1 = wall_seconds();

        let (_mindeg, maxdeg, _sumdeg) = self.rowcounts().fold((self.numcols, 0, 0), |acc, x| {
            (acc.0.min(x), acc.1.max(x), acc.2 + x)
        });
        let maxdeg = self.graph.convey.reduce_max(maxdeg);

        // choose a value for delta, the bucket width
        let delta;
        if let Some(d) = forced_delta {
            delta = d;
        } else {
            delta = 1.0 / (maxdeg as f64);
        }
        println!(
            "delta_stepping: nvtxs = {}, nedges = {}, delta = {}",
            self.numrows, 
            self.offset[self.numrows], 
            delta
        );
        
        // initialize buckets, activated flags, etc.
        let mut searcher = BucketSearcher::new(&self, delta);

        // use relax to set tent(source) to 0, which also puts it in bucket 0
        searcher.relax(Request{w_g: source, dist: 0.0});

        searcher
            .dump(20, "trace.out", "after relax source", vec![source])
            .expect("bucket dump failed");

        // outer loop: for each nonempty bucket in order ...
        let mut outer = 0;
        let mut active_bucket_if_any = Some(0);
        while let Some(active_bucket) = active_bucket_if_any {
            println!("\nouter loop iteration {}: active_bucket = {}", outer, active_bucket);

            let mut removed: Vec<usize> = Vec::new(); // vertices removed from active bucket, R in paper
        
            // middle loop: while active bucket (B[i] in paper) is not empty ...
            let mut phase = 0;
            while searcher.global_bucket_size(active_bucket) > 0 { 
                println!(
                    "middle loop iteration {}: active_bucket has {} vtxs", 
                    phase, 
                    searcher.bucket_size[active_bucket]
                );

                // find light edges with tails in active bucket;
                // empty active bucket, keeping a set "removed" of unique vtxs removed from this bucket;
                // relax light edges, which may put some removed and other vtxs into the active bucket.

                let requests = searcher.find_light_requests(active_bucket); 
                removed.append(&mut searcher.newly_active_vertices(active_bucket));
                searcher.empty_bucket(active_bucket);
                searcher.relax_requests(requests);

                searcher
                    .dump(20, "trace.out", "end of middle iter", vec![outer, phase])
                    .expect("bucket dump failed");
                phase += 1;
                
            } // end of middle looop

            // relax heavy edges with tails in removed set, which cannot add vtxs to active bucket
            let requests = searcher.find_heavy_requests(removed);
            searcher.relax_requests(requests);

            searcher
                .dump(20, "trace.out", "end of outer iter", vec![outer])
                .expect("bucket dump failed");
            outer += 1;

            active_bucket_if_any = searcher.next_nonempty_bucket(active_bucket);

        } // end of outer loop

        println!("\nDid {} iterations of outer loop.", outer);

        // return the info struct, which will now own the distance array
        SsspInfo {
            distance: searcher.tentative_dist,
            source: source,
            laptime: wall_seconds() - t1,
        }
    }

    /// check the result of delta stepping
    ///
    /// # Arguments
    /// * info data from the run to check
    /// * dump_files debugging flag
    fn check_result(&self, info: &SsspInfo, dump_files: bool) -> bool {
        println!("\ncheck_result: source is {}, dump_files is {}", info.source, dump_files);
        if dump_files {
            info.dump(20, "dist.out").expect("info dump error");
        }
        let mut unreachable = 0;
        let mut max_dist: f64 = 0.0;
        let mut sum_dist: f64 = 0.0;
        for v in 0..self.numrows {
            if info.distance[v].is_finite() {
                max_dist = f64::max(max_dist, info.distance[v]);
                sum_dist += info.distance[v];
            } else {
                unreachable += 1;
            }
        }
        println!(
            "unreachable vertices: {}; max finite distance: {}; avg finite distance: {}", 
            unreachable, 
            max_dist, 
            sum_dist/((self.numrows - unreachable) as f64)
        );
        true
    }
}
