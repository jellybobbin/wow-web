use crate::core::router::router::{Param, Params};
use http_types::{Request,Response,StatusCode};
use async_std::future::Future;
// use std::fmt::Debug;
use std::mem;
use std::str;

fn min(a: usize, b: usize) -> usize {
    if a <= b {
        return a;
    }
    b
}

fn count_params(path: &[u8]) -> u8 {
    let mut n = 0;
    for &c in path {
        if c != b':' && c != b'*' {
            continue;
        }
        n += 1;
    }
    if n > 255 {
        return 255;
    }
    n as u8
}


pub type BoxFut = Box<dyn Future<output = Response> + Send>;


pub trait Handle {
    fn handle(&self, req: Request, ps: Params) -> BoxFut;
}

impl<F> Handle for F
    where
        F: Fn(Request, Params) -> BoxFut,
{
    fn handle(&self, req: Request, ps: Params) -> BoxFut {
        (*self)(req, ps)
    }
}

pub type Handler = Box<Handle + Send + Sync>;


#[derive(PartialEq, Clone, Debug, PartialOrd)]
pub enum NodeType {
    Static,
    Root,
    Param,
    CatchAll,
}

#[derive(Debug, Clone)]
pub struct Node {
    path: Vec<u8>,
    wild_child: bool,
    n_type: NodeType,
    max_params: u8,
    indices: Vec<u8>,
    children: Vec<Box<Node>>,
    handle: Option<Handler>,
    priority: u32,
}

impl<T> Node<T> {
    pub fn new() -> Node<T> {
        Node {
            path: Vec::new(),
            wild_child: false,
            n_type: NodeType::Static,
            max_params: 0,
            indices: Vec::new(),
            children: Vec::new(),
            handle: None,
            priority: 0,
        }
    }

    /// increments priority of the given child and reorders if necessary
    fn increment_child_prio(&mut self, pos: usize) -> usize {
        self.children[pos].priority += 1;
        let prio = self.children[pos].priority;
        // adjust position (move to front)
        let mut new_pos = pos;

        while new_pos > 0 && self.children[new_pos - 1].priority < prio {
            // swap node positions
            self.children.swap(new_pos - 1, new_pos);
            new_pos -= 1;
        }

        // build new index char string
        if new_pos != pos {
            self.indices = [
                &self.indices[..new_pos],    // unchanged prefix, might be empty
                &self.indices[pos..pos + 1], // the index char we move
                &self.indices[new_pos..pos], // rest without char at 'pos'
                &self.indices[pos + 1..],    // rest without char at 'pos'
            ].concat();
        }

        new_pos
    }

    /// addRoute adds a node with the given handle to the path.
    /// Not concurrency-safe!
    pub fn add_route(&mut self, path: &str, handle: T) {
        let full_path = path.clone();
        let path = path.as_ref();
        self.priority += 1;
        let num_params = count_params(path);

        // non-empty tree
        if self.path.len() > 0 || self.children.len() > 0 {
            self.add_route_loop(num_params, path, full_path, handle);
        } else {
            // Empty tree
            self.insert_child(num_params, path, full_path, handle);
            self.n_type = NodeType::Root;
        }
    }

    fn add_route_loop(&mut self, num_params: u8, mut path: &[u8], full_path: &str, handle: T) {
        // Update max_params of the current node
        if num_params > self.max_params {
            self.max_params = num_params;
        }

        // Find the longest common prefix.
        // This also implies that the common prefix contains no ':' or '*'
        // since the existing key can't contain those chars.
        let mut i = 0;
        let max = min(path.len(), self.path.len());

        while i < max && path[i] == self.path[i] {
            i += 1;
        }

        // Split edge
        if i < self.path.len() {
            let mut child = Node {
                path: self.path[i..].to_vec(),
                wild_child: self.wild_child,
                n_type: NodeType::Static,
                indices: self.indices.clone(),
                children: Vec::new(),
                handle: self.handle.take(),
                priority: self.priority - 1,

                max_params: 0,
            };

            mem::swap(&mut self.children, &mut child.children);

            // Update max_params (max of all children)
            for c in &child.children {
                if c.max_params > child.max_params {
                    child.max_params = c.max_params;
                }
            }

            self.children = vec![Box::new(child)];
            self.indices = vec![self.path[i]];
            self.path = path[..i].to_vec();
            self.wild_child = false;
        }

        // Make new node a child of this node
        if i < path.len() {
            path = &path[i..];

            if self.wild_child {
                // *n = * {n}.children[0].clone();
                return self.children[0].is_wild_child(num_params, path, full_path, handle);
            }

            let c = path[0];

            // slash after param
            if self.n_type == NodeType::Param && c == b'/' && self.children.len() == 1 {
                self.children[0].priority += 1;
                return self.children[0].add_route_loop(num_params, path, full_path, handle);
            }

            // Check if a child with the next path byte exists
            for mut i in 0..self.indices.len() {
                if c == self.indices[i] {
                    i = self.increment_child_prio(i);
                    return self.children[i].add_route_loop(num_params, path, full_path, handle);
                }
            }

            // Otherwise insert it
            if c != b':' && c != b'*' {
                self.indices.push(c);

                let len = self.indices.len();

                let child: Box<Node<T>> = Box::new(Node {
                    path: Vec::new(),

                    wild_child: false,

                    n_type: NodeType::Static,

                    max_params: num_params,

                    indices: Vec::new(),

                    children: Vec::new(),

                    handle: None,

                    priority: 0,
                });

                self.children.push(child);

                let i = self.increment_child_prio(len - 1);

                return self.children[i].insert_child(num_params, path, full_path, handle);
            }

            return self.insert_child(num_params, path, full_path, handle);
        } else if i == path.len() {
            // Make node a (in-path) leaf
            if self.handle.is_some() {
                panic!("a handle is already registered for path '{}'", full_path);
            }

            self.handle = Some(handle);
        }

        return;
    }

    fn is_wild_child(&mut self, mut num_params: u8, path: &[u8], full_path: &str, handle: T) {
        self.priority += 1;

        // Update maxParams of the child node

        if num_params > self.max_params {
            self.max_params = num_params;
        }

        num_params -= 1;

        // Check if the wildcard matches

        if path.len() >= self.path.len()
            && self.path == &path[..self.path.len()]
            // Check for longer wildcard, e.g. :name and :names
            && (self.path.len() >= path.len() || path[self.path.len()] == b'/')
        {
            self.add_route_loop(num_params, path, full_path, handle);
        } else {
            // Wildcard conflict
            let path_seg = if self.n_type == NodeType::CatchAll {
                str::from_utf8(path).unwrap()
            } else {
                str::from_utf8(path)
                    .unwrap()
                    .splitn(2, '/')
                    .into_iter()
                    .next()
                    .unwrap()
            };

            let prefix = [
                &full_path[..full_path.find(path_seg).unwrap()],
                str::from_utf8(&self.path).unwrap(),
            ].concat();

            panic!("'{}' in new path '{}' conflicts with existing wildcard '{}' in existing prefix '{}'", path_seg, full_path, str::from_utf8(&self.path).unwrap(), prefix);
        }
    }

    fn insert_child(&mut self, num_params: u8, path: &[u8], full_path: &str, handle: T) {
        self.insert_child_loop(0, 0, num_params, path, full_path, handle);
    }

    fn insert_child_loop(
        &mut self,
        mut offset: usize,
        mut i: usize,
        mut num_params: u8,
        path: &[u8],
        full_path: &str,
        handle: T,
    ) {
        if num_params > 0 {
            let max = path.len();
            let c = path[i];

            // find prefix until first wildcard (beginning with ':'' or '*'')
            if c != b':' && c != b'*' {
                return self.insert_child_loop(offset, i + 1, num_params, path, full_path, handle);
            }

            // find wildcard end (either '/' or path end)
            let mut end = i + 1;
            while end < max && path[end] != b'/' {
                match path[end] {
                    // the wildcard name must not contain ':' and '*'
                    b':' | b'*' => panic!(
                        "only one wildcard per path segment is allowed, has: '{}' in path '{}'",
                        str::from_utf8(&path[i..]).unwrap(),
                        full_path
                    ),
                    _ => end += 1,
                }
            }

            // println!("self path: {}", str::from_utf8(&self.path).unwrap());
            // println!("temp path: {}", str::from_utf8(path).unwrap());
            // println!("self {:?}", self.children[0]);
            // println!("self {:?}", self.children.len());

            // check if this Node existing children which would be
            // unreachable if we insert the wildcard here
            if self.children.len() > 0 {
                panic!(
                    "wildcard route '{}' conflicts with existing children in path '{}'",
                    str::from_utf8(&path[i..end]).unwrap(),
                    full_path
                )
            }

            // check if the wildcard has a name
            if end - i < 2 {
                panic!(
                    "wildcards must be named with a non-empty name in path '{}'",
                    full_path
                );
            }

            if c == b':' {
                // Param
                // split path at the beginning of the wildcard
                if i > 0 {
                    self.path = path[offset..i].to_vec();
                    offset = i;
                }

                let child = Box::new(Node {
                    path: Vec::new(),
                    wild_child: false,
                    n_type: NodeType::Param,
                    max_params: num_params,
                    indices: Vec::new(),
                    children: Vec::new(),
                    handle: None,
                    priority: 0,
                });

                self.children = vec![child];
                self.wild_child = true;

                self.children[0].priority += 1;
                num_params -= 1;

                if end < max {
                    self.children[0].path = path[offset..end].to_vec();
                    offset = end;

                    let child: Box<Node<T>> = Box::new(Node {
                        path: Vec::new(),
                        wild_child: false,
                        n_type: NodeType::Static,
                        max_params: num_params,
                        indices: Vec::new(),
                        children: Vec::new(),
                        handle: None,
                        priority: 1,
                    });

                    self.children[0].children.push(child);
                    self.children[0].children[0].insert_child_loop(
                        offset,
                        i + 1,
                        num_params,
                        path,
                        full_path,
                        handle,
                    );
                } else {
                    self.children[0].insert_child_loop(
                        offset,
                        i + 1,
                        num_params,
                        path,
                        full_path,
                        handle,
                    );
                }
            } else {
                // CatchAll
                if end != max || num_params > 1 {
                    panic!(
                        "catch-all routes are only allowed at the end of the path in path '{}'",
                        full_path
                    );
                }

                if self.path.len() > 0 && self.path[self.path.len() - 1] == b'/' {
                    panic!(
                        "catch-all conflicts with existing handle for the path segment root in path '{}'", 
                        full_path
                    );
                }

                // currently fixed width 1 for '/'
                i -= 1;
                if path[i] != b'/' {
                    panic!("no / before catch-all in path '{}'", full_path);
                }

                self.path = path[offset..i].to_vec();

                // first node: catchAll node with empty path
                let child = Box::new(Node {
                    path: Vec::new(),
                    wild_child: true,
                    n_type: NodeType::CatchAll,
                    max_params: 1,
                    indices: Vec::new(),
                    children: Vec::new(),
                    handle: None,
                    priority: 0,
                });

                self.children = vec![child];

                self.indices = vec![path[i]];

                self.children[0].priority += 1;

                // second node: node holding the variable
                let child: Box<Node<T>> = Box::new(Node {
                    path: path[i..].to_vec(),
                    wild_child: false,
                    n_type: NodeType::CatchAll,
                    max_params: 1,
                    indices: Vec::new(),
                    children: Vec::new(),
                    handle: Some(handle),
                    priority: 1,
                });

                self.children[0].children.push(child);

                return;
            }
        } else {
            // insert remaining path part and handle to the leaf
            self.path = path[offset..].to_vec();
            self.handle = Some(handle);
        }
    }

    /// Returns the handle registered with the given path (key). The values of
    /// wildcards are saved to a map.
    /// If no handle can be found, a TSR (trailing slash redirect) recommendation is
    /// made if a handle exists with an extra (without the) trailing slash for the
    /// given path.
    pub fn get_value(&self, path: &str) -> (Option<&T>, Params, bool) {
        // let mut handle = None;
        self.get_value_loop(path.as_ref(), Params::new())
    }

    /// outer loop for walking the tree
    fn get_value_loop(&self, mut path: &[u8], p: Params) -> (Option<&T>, Params, bool) {
        if path.len() > self.path.len() {
            if self.path == &path[..self.path.len()] {
                path = &path[self.path.len()..];
                // If this node does not have a wildcard (param or catchAll)
                // child,  we can just look up the next child node and continue
                // to walk down the tree
                if !self.wild_child {
                    let c = path[0];
                    for i in 0..self.indices.len() {
                        if c == self.indices[i] {
                            return self.children[i].get_value_loop(path, p);
                        }
                    }
                    // Nothing found.
                    // We can recommend to redirect to the same URL without a
                    // trailing slash if a leaf exists for that path.
                    let tsr = path == [b'/'] && self.handle.is_some();
                    return (None, p, tsr);
                }

                // handle wildcard child
                return self.children[0].handle_wildcard_child(path, p);
            }
        } else if self.path == path {
            // We should have reached the node containing the handle.
            // Check if this node has a handle registered.
            if self.handle.is_some() {
                return (self.handle.as_ref(), p, false);
            }

            if path == [b'/'] && self.wild_child && self.n_type != NodeType::Root {
                // tsr = true;
                return (self.handle.as_ref(), p, true);
            }

            // No handle found. Check if a handle for this path + a
            // trailing slash exists for trailing slash recommendation
            for i in 0..self.indices.len() {
                if self.indices[i] == b'/' {
                    let tsr = (self.path.len() == 1 && self.children[i].handle.is_some())
                        || (self.children[i].n_type == NodeType::CatchAll
                            && self.children[i].children[0].handle.is_some());
                    return (self.handle.as_ref(), p, tsr);
                }
            }

            return (self.handle.as_ref(), p, false);
        }

        // Nothing found. We can recommend to redirect to the same URL with an
        // extra trailing slash if a leaf exists for that path
        let tsr = (path == [b'/'])
            || (self.path.len() == path.len() + 1
                && self.path[path.len()] == b'/'
                && path == &self.path[..self.path.len() - 1]
                && self.handle.is_some());

        return (None, p, tsr);
    }

    fn handle_wildcard_child(&self, mut path: &[u8], mut p: Params) -> (Option<&T>, Params, bool) {
        match self.n_type {
            NodeType::Param => {
                // find param end (either '/' or path end)
                let mut end = 0;
                while end < path.len() && path[end] != b'/' {
                    end += 1;
                }

                // save param value
                if p.is_empty() {
                    // lazy allocation
                    p = Params(Vec::with_capacity(self.max_params as usize));
                }

                p.push(Param {
                    key: String::from_utf8(self.path[1..].to_vec()).unwrap(),
                    value: String::from_utf8(path[..end].to_vec()).unwrap(),
                });

                // we need to go deeper!
                if end < path.len() {
                    if self.children.len() > 0 {
                        path = &path[end..];

                        return self.children[0].get_value_loop(path, p);
                    }

                    // ... but we can't
                    let tsr = path.len() == end + 1;
                    return (None, p, tsr);
                }

                if self.handle.is_some() {
                    return (self.handle.as_ref(), p, false);
                } else if self.children.len() == 1 {
                    // No handle found. Check if a handle for this path + a
                    // trailing slash exists for TSR recommendation
                    let tsr = self.children[0].path == &[b'/'] && self.children[0].handle.is_some();
                    return (None, p, tsr);
                }

                return (None, p, false);
            }
            NodeType::CatchAll => {
                // save param value
                if p.is_empty() {
                    // lazy allocation
                    p = Params(Vec::with_capacity(self.max_params as usize));
                }

                p.push(Param {
                    key: String::from_utf8(self.path[2..].to_vec()).unwrap(),
                    value: String::from_utf8(path.to_vec()).unwrap(),
                });

                return (self.handle.as_ref(), p, false);
            }
            _ => panic!("invalid node type"),
        }
    }

    /// Makes a case-insensitive lookup of the given path and tries to find a handler.
    /// It can optionally also fix trailing slashes.
    /// It returns the case-corrected path and a bool indicating whether the lookup
    /// was successful.
    pub fn find_case_insensitive_path(
        &self,
        path: &str,
        fix_trailing_slash: bool,
    ) -> (String, bool) {
        let mut ci_path = Vec::with_capacity(path.len() + 1);
        let found = self.find_case_insensitive_path_rec(
            path.as_bytes(),
            path.to_ascii_lowercase().as_bytes(),
            &mut ci_path,
            [0; 4],
            fix_trailing_slash,
        );
        (String::from_utf8(ci_path).unwrap(), found)
    }

    /// recursive case-insensitive lookup function used by n.findCaseInsensitivePath
    fn find_case_insensitive_path_rec(
        &self,
        mut path: &[u8],
        mut lo_path: &[u8],
        ci_path: &mut Vec<u8>,
        mut rb: [u8; 4],
        fix_trailing_slash: bool,
    ) -> bool {
        // println!("{:?}", self.path);
        // let n_path = str::from_utf8(&self.path).expect("ivalid utf8").to_lowercase();
        // let lo_n_path = n_path.as_bytes();
        let lo_n_path: Vec<u8> = self.path.iter().map(|u| u.to_ascii_lowercase()).collect();

        if lo_path.len() >= lo_n_path.len()
            && (lo_n_path.len() == 0 || lo_path[1..lo_n_path.len()] == lo_n_path[1..])
        {
            // println!("self.path = {}", str::from_utf8(&self.path).unwrap());
            ci_path.append(&mut self.path.clone());

            path = &path[self.path.len()..];

            if path.len() > 0 {
                let lo_old = lo_path.clone();
                lo_path = &lo_path[lo_n_path.len()..];

                // If this node does not have a wildcard (param or catchAll) child,
                // we can just look up the next child node and continue to walk down
                // the tree
                if !self.wild_child {
                    // skip rune bytes already processed
                    rb = shift_n_rune_bytes(rb, lo_n_path.len());

                    if rb[0] != 0 {
                        // old rune not finished
                        for i in 0..self.indices.len() {
                            if self.indices[i] == rb[0] {
                                // continue with child node
                                return self.children[i].find_case_insensitive_path_rec(
                                    path,
                                    lo_path,
                                    ci_path,
                                    rb,
                                    fix_trailing_slash,
                                );
                            }
                        }
                    } else {
                        // process a new rune
                        let mut rv = 0 as char;

                        // find rune start
                        // runes are up to 4 byte long,
                        // -4 would definitely be another rune
                        let mut off = 0;
                        // println!("loold {:?}", lo_old);
                        for j in 0..min(lo_n_path.len(), 3) {
                            let i = lo_n_path.len() - j;
                            if rune_start(lo_old[i]) {
                                // read rune from cached lowercase path
                                rv = str::from_utf8(&lo_old[i..])
                                    .unwrap()
                                    .chars()
                                    .next()
                                    .unwrap();
                                off = j;
                                break;
                            }
                        }
                        // println!("rv = {}, off = {}", rv, off);
                        // calculate lowercase bytes of current rune
                        rv.encode_utf8(&mut rb);

                        // skipp already processed bytes
                        rb = shift_n_rune_bytes(rb, off);
                        // println!("rb = {:?}", rb);
                        for i in 0..self.indices.len() {
                            // lowercase matches
                            if self.indices[i] == rb[0] {
                                // must use a recursive approach since both the
                                // uppercase byte and the lowercase byte might exist
                                // as an index
                                let found = self.children[i].find_case_insensitive_path_rec(
                                    path,
                                    lo_path,
                                    ci_path,
                                    rb,
                                    fix_trailing_slash,
                                );

                                if found {
                                    // println!("cipah = {}", str::from_utf8(&ci_path).unwrap());
                                    return true;
                                }
                                if ci_path.len() > self.children[i].path.len() {
                                    let prev_len = ci_path.len() - self.children[i].path.len();
                                    ci_path.truncate(prev_len);
                                }

                                break;
                            }
                        }

                        // same for uppercase rune, if it differs
                        let up = rv.to_ascii_uppercase();
                        if up != rv {
                            up.encode_utf8(&mut rb);
                            rb = shift_n_rune_bytes(rb, off);

                            for i in 0..self.indices.len() {
                                if self.indices[i] == rb[0] {
                                    return self.children[i].find_case_insensitive_path_rec(
                                        path,
                                        lo_path,
                                        ci_path,
                                        rb,
                                        fix_trailing_slash,
                                    );
                                }
                            }
                        }
                    }

                    // Nothing found. We can recommend to redirect to the same URL
				    // without a trailing slash if a leaf exists for that path
                    return fix_trailing_slash && path == [b'/'] && self.handle.is_some();
                }

                return self.children[0].find_case_insensitive_path_rec_match(
                    path,
                    lo_path,
                    ci_path,
                    rb,
                    fix_trailing_slash,
                );
            } else {
                if self.handle.is_some() {
                    return true;
                }

                if fix_trailing_slash {
                    for i in 0..self.indices.len() {
                        if self.indices[i] == b'/' {
                            if (self.children[i].path.len() == 1
                                && self.children[i].handle.is_some())
                                || (self.children[i].n_type == NodeType::CatchAll
                                    && self.children[i].children[0].handle.is_some())
                            {
                                ci_path.push(b'/');
                                return true;
                            }
                            return false;
                        }
                    }
                }
                return false;
            }
        }

        if fix_trailing_slash {
            if path == [b'/'] {
                return true;
            }
            if lo_path.len() + 1 == lo_n_path.len()
                && lo_n_path[lo_path.len()] == b'/'
                && lo_path[1..] == lo_n_path[1..lo_path.len()]
                && self.handle.is_some()
            {
                ci_path.append(&mut self.path.clone());
                return true;
            }
        }

        false
    }

    /// recursive case-insensitive lookup function used by n.findCaseInsensitivePath
    fn find_case_insensitive_path_rec_match(
        &self,
        mut path: &[u8],
        mut lo_path: &[u8],
        ci_path: &mut Vec<u8>,
        rb: [u8; 4],
        fix_trailing_slash: bool,
    ) -> bool {
        match self.n_type {
            NodeType::Param => {
                let mut k = 0;
                while k < path.len() && path[k] != b'/' {
                    k += 1;
                }
                let mut path_k = path[..k].to_vec();
                ci_path.append(&mut path_k);

                if k < path.len() {
                    if self.children.len() > 0 {
                        lo_path = &lo_path[k..];
                        path = &path[k..];

                        return self.children[0].find_case_insensitive_path_rec(
                            path,
                            lo_path,
                            ci_path,
                            rb,
                            fix_trailing_slash,
                        );
                    }

                    if fix_trailing_slash && path.len() == k + 1 {
                        return true;
                    }
                    return false;
                }

                if self.handle.is_some() {
                    return true;
                } else if fix_trailing_slash && self.children.len() == 1 {
                    if self.children[0].path == [b'/'] && self.children[0].handle.is_some() {
                        ci_path.push(b'/');
                        return true;
                    }
                }

                return false;
            }
            NodeType::CatchAll => {
                ci_path.append(&mut path.to_vec());
                return true;
            }
            _ => panic!("invalid node type"),
        }
    }
}

fn shift_n_rune_bytes(rb: [u8; 4], n: usize) -> [u8; 4] {
    match n {
        0 => rb,
        1 => [rb[1], rb[2], rb[3], 0],
        2 => [rb[2], rb[3], 0, 0],
        3 => [rb[3], 0, 0, 0],
        _ => [0; 4],
    }
}

/// This function is ported from go.
fn rune_start(b: u8) -> bool {
    b & 0xC0 != 0x80
}

#[cfg(test)]
mod tests {
    use super::*;
    // use hyper::{Body, Request, Response};
    use crate::router::Params;
    use std::panic;
    use std::sync::Mutex;

    // fn print_children() {}

    struct TestRequest<'a> {
        path: &'a str,
        nil_handler: bool,
        route: &'a str,
        ps: Params,
    }

    impl<'a> TestRequest<'a> {
        pub fn new(
            path: &'a str,
            nil_handler: bool,
            route: &'a str,
            ps: Params,
        ) -> TestRequest<'a> {
            TestRequest {
                path,
                nil_handler,
                route,
                ps,
            }
        }
    }

    type TestRequests<'a> = Vec<TestRequest<'a>>;

    fn check_requests<T: Fn() -> String>(tree: &mut Node<T>, requests: TestRequests) {
        for request in requests {
            let (handler, ps, _) = tree.get_value(request.path);

            if handler.is_none() {
                if !request.nil_handler {
                    panic!(
                        "handle mismatch for route '{}': Expected non-nil handle",
                        request.path
                    );
                }
            } else if request.nil_handler {
                panic!(
                    "handle m ismatch for route '{}': Expected nil handle",
                    request.path
                );
            } else {
                match handler {
                    Some(h) => {
                        let res = h();
                        if res != request.route {
                            panic!(
                                "handle mismatch for route '{}': Wrong handle ({} != {})",
                                request.path, res, request.route
                            );
                        }
                    }
                    None => {
                        panic!("handle not found");
                    }
                }
            }

            if ps != request.ps {
                panic!("Params mismatch for route '{}'", request.path);
            }
        }
    }

    fn check_priorities<T: Fn() -> String>(n: &mut Node<T>) -> u32 {
        // println!("{}", str::from_utf8(&n.path).unwrap());
        let mut prio: u32 = 0;
        for i in 0..n.children.len() {
            prio += check_priorities(&mut *n.children[i]);
        }

        if n.handle.is_some() {
            prio += 1;
        }

        if n.priority != prio {
            panic!(
                "priority mismatch for node '{}': is {}, should be {}",
                str::from_utf8(&n.path).unwrap(),
                n.priority,
                prio
            )
        }

        prio
    }

    fn check_max_params<T: Fn() -> String>(n: &mut Node<T>) -> u8 {
        let mut max_params: u8 = 0;
        for i in 0..n.children.len() {
            let params = check_max_params(&mut *n.children[i]);

            if params > max_params {
                max_params = params;
            }
        }

        if n.n_type > NodeType::Root && !n.wild_child {
            max_params += 1;
        }

        if n.max_params != max_params {
            panic!(
                "maxParams mismatch for node '{}': is {}, should be {}",
                str::from_utf8(&n.path).unwrap(),
                n.max_params,
                max_params,
            )
        }

        max_params
    }

    fn fake_handler(val: &'static str) -> impl Fn() -> String {
        move || val.to_string()
    }

    #[test]
    fn test_count_params() {
        assert_eq!(
            2,
            count_params("/path/:param1/static/*catch-all".as_bytes())
        );
        assert_eq!(255, count_params("/:param".repeat(256).as_bytes()));
    }

    #[test]
    fn test_tree_add_and_get() {
        let mut tree = Node::new();

        let routes = vec![
            "/hi",
            "/contact",
            "/co",
            "/c",
            "/a",
            "/ab",
            "/doc/",
            "/doc/go_faq.html",
            "/doc/go1.html",
            "/α",
            "/β",
        ];

        for route in routes {
            tree.add_route(route, fake_handler(route));
        }

        check_requests(
            &mut tree,
            vec![
                TestRequest::new("/a", false, "/a", Params::new()),
                TestRequest::new("/", true, "", Params::new()),
                TestRequest::new("/hi", false, "/hi", Params::new()),
                TestRequest::new("/contact", false, "/contact", Params::new()),
                TestRequest::new("/co", false, "/co", Params::new()),
                TestRequest::new("/con", true, "", Params::new()), // key mismatch
                TestRequest::new("/cona", true, "", Params::new()), // key mismatch
                TestRequest::new("/no", true, "", Params::new()),  // no matching child
                TestRequest::new("/ab", false, "/ab", Params::new()),
                TestRequest::new("/α", false, "/α", Params::new()),
                TestRequest::new("/β", false, "/β", Params::new()),
            ],
        );

        check_priorities(&mut tree);
        check_max_params(&mut tree);
    }

    #[test]
    fn test_tree_wildcard() {
        let mut tree = Node::new();

        let routes = vec![
            "/",
            "/cmd/:tool/:sub",
            "/cmd/:tool/",
            "/src/*filepath",
            "/search/",
            "/search/:query",
            "/user_:name",
            "/user_:name/about",
            "/files/:dir/*filepath",
            "/doc/",
            "/doc/go_faq.html",
            "/doc/go1.html",
            "/info/:user/public",
            "/info/:user/project/:project",
        ];

        for route in routes {
            tree.add_route(route, fake_handler(route));
        }

        check_requests(
            &mut tree,
            vec![
                TestRequest::new("/", false, "/", Params::new()),
                TestRequest::new(
                    "/cmd/test/",
                    false,
                    "/cmd/:tool/",
                    Params(vec![Param::new("tool", "test")]),
                ),
                TestRequest::new(
                    "/cmd/test",
                    true,
                    "",
                    Params(vec![Param::new("tool", "test")]),
                ),
                TestRequest::new(
                    "/cmd/test/3",
                    false,
                    "/cmd/:tool/:sub",
                    Params(vec![Param::new("tool", "test"), Param::new("sub", "3")]),
                ),
                TestRequest::new(
                    "/src/",
                    false,
                    "/src/*filepath",
                    Params(vec![Param::new("filepath", "/")]),
                ),
                TestRequest::new(
                    "/src/some/file.png",
                    false,
                    "/src/*filepath",
                    Params(vec![Param::new("filepath", "/some/file.png")]),
                ),
                TestRequest::new("/search/", false, "/search/", Params::new()),
                TestRequest::new(
                    "/search/someth!ng+in+ünìcodé",
                    false,
                    "/search/:query",
                    Params(vec![Param::new("query", "someth!ng+in+ünìcodé")]),
                ),
                TestRequest::new(
                    "/search/someth!ng+in+ünìcodé/",
                    true,
                    "",
                    Params(vec![Param::new("query", "someth!ng+in+ünìcodé")]),
                ),
                TestRequest::new(
                    "/user_gopher",
                    false,
                    "/user_:name",
                    Params(vec![Param::new("name", "gopher")]),
                ),
                TestRequest::new(
                    "/user_gopher/about",
                    false,
                    "/user_:name/about",
                    Params(vec![Param::new("name", "gopher")]),
                ),
                TestRequest::new(
                    "/files/js/inc/framework.js",
                    false,
                    "/files/:dir/*filepath",
                    Params(vec![
                        Param::new("dir", "js"),
                        Param::new("filepath", "/inc/framework.js"),
                    ]),
                ),
                TestRequest::new(
                    "/info/gordon/public",
                    false,
                    "/info/:user/public",
                    Params(vec![Param::new("user", "gordon")]),
                ),
                TestRequest::new(
                    "/info/gordon/project/go",
                    false,
                    "/info/:user/project/:project",
                    Params(vec![
                        Param::new("user", "gordon"),
                        Param::new("project", "go"),
                    ]),
                ),
            ],
        );

        check_priorities(&mut tree);
        check_max_params(&mut tree);
    }

    // path: &str, conflict: bool
    type TestRoute = (&'static str, bool);

    fn test_routes(routes: Vec<TestRoute>) {
        let tree = Mutex::new(Node::new());
        // let mut tree = Node::new();

        for route in routes {
            let recv = panic::catch_unwind(|| {
                let mut guard = match tree.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.add_route(route.0, ());
            });

            if route.1 {
                if recv.is_ok() {
                    panic!("no panic for conflicting route '{}'", route.0);
                }
            } else if recv.is_err() {
                panic!("unexpected panic for route '{}': {:?}", route.0, recv);
            }
        }
    }

    #[test]
    fn test_tree_wildcard_conflict() {
        let routes = vec![
            ("/cmd/:tool/:sub", false),
            ("/cmd/vet", true),
            ("/src/*filepath", false),
            ("/src/*filepathx", true),
            ("/src/", true),
            ("/src1/", false),
            ("/src1/*filepath", true),
            ("/src2*filepath", true),
            ("/search/:query", false),
            ("/search/invalid", true),
            ("/user_:name", false),
            ("/user_x", true),
            // ("/user_:name", false),
            ("/user_:name", true), // Rust is different. Nil handler was not allowed. Or maybe it is a feature?
            ("/id:id", false),
            ("/id/:id", true),
        ];
        test_routes(routes);
    }

    #[test]
    fn test_tree_child_conflict() {
        let routes = vec![
            ("/cmd/vet", false),
            ("/cmd/:tool/:sub", true),
            ("/src/AUTHORS", false),
            ("/src/*filepath", true),
            ("/user_x", false),
            ("/user_:name", true),
            ("/id/:id", false),
            ("/id:id", true),
            ("/:id", true),
            ("/*filepath", true),
        ];

        test_routes(routes);
    }

    #[test]
    fn test_tree_duplicate_path() {
        let tree = Mutex::new(Node::new());

        let routes = vec![
            "/",
            "/doc/",
            "/src/*filepath",
            "/search/:query",
            "/user_:name",
        ];

        for route in routes {
            let mut recv = panic::catch_unwind(|| {
                let mut guard = match tree.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.add_route(route, fake_handler(route));
            });

            if recv.is_err() {
                panic!("panic inserting route '{}': {:?}", route, recv);
            }

            recv = panic::catch_unwind(|| {
                let mut guard = match tree.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.add_route(route, fake_handler(route));
            });

            if recv.is_ok() {
                panic!("no panic while inserting duplicate route '{}'", route);
            }
        }

        check_requests(
            &mut tree.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![
                TestRequest::new("/", false, "/", Params::new()),
                TestRequest::new("/doc/", false, "/doc/", Params::new()),
                TestRequest::new(
                    "/src/some/file.png",
                    false,
                    "/src/*filepath",
                    Params(vec![Param::new("filepath", "/some/file.png")]),
                ),
                TestRequest::new(
                    "/search/someth!ng+in+ünìcodé",
                    false,
                    "/search/:query",
                    Params(vec![Param::new("query", "someth!ng+in+ünìcodé")]),
                ),
                TestRequest::new(
                    "/user_gopher",
                    false,
                    "/user_:name",
                    Params(vec![Param::new("name", "gopher")]),
                ),
            ],
        );
    }

    #[test]
    fn test_empty_wildcard_name() {
        let tree = Mutex::new(Node::new());
        let routes = vec!["/user:", "/user:/", "/cmd/:/", "/src/*"];

        for route in routes {
            let recv = panic::catch_unwind(|| {
                let mut guard = match tree.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.add_route(route, fake_handler(route));
            });

            if recv.is_ok() {
                panic!(
                    "no panic while inserting route with empty wildcard name '{}",
                    route
                );
            }
        }
    }

    #[test]
    fn test_tree_catch_all_conflict() {
        let routes = vec![
            ("/src/*filepath/x", true),
            ("/src2/", false),
            ("/src2/*filepath/x", true),
        ];

        test_routes(routes);
    }

    #[test]
    fn test_tree_catch_all_conflict_root() {
        let routes = vec![("/", false), ("/*filepath", true)];

        test_routes(routes);
    }

    #[test]
    fn test_tree_double_wildcard() {
        let panic_msg = "only one wildcard per path segment is allowed";
        let routes = vec!["/:foo:bar", "/:foo:bar/", "/:foo*bar"];

        for route in routes {
            let tree = Mutex::new(Node::new());
            let recv = panic::catch_unwind(|| {
                let mut guard = match tree.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.add_route(route, fake_handler(route));
            });

            // [TODO] Not strict enough
            if recv.is_ok() {
                panic!(panic_msg);
            }
        }
    }

    #[test]
    fn test_tree_trailing_slash_redirect() {
        let tree = Mutex::new(Node::new());
        let routes = vec![
            "/hi",
            "/b/",
            "/search/:query",
            "/cmd/:tool/",
            "/src/*filepath",
            "/x",
            "/x/y",
            "/y/",
            "/y/z",
            "/0/:id",
            "/0/:id/1",
            "/1/:id/",
            "/1/:id/2",
            "/aa",
            "/a/",
            "/admin",
            "/admin/:category",
            "/admin/:category/:page",
            "/doc",
            "/doc/go_faq.html",
            "/doc/go1.html",
            "/no/a",
            "/no/b",
            "/api/hello/:name",
        ];

        for route in routes {
            let recv = panic::catch_unwind(|| {
                let mut guard = match tree.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.add_route(route, fake_handler(route));
            });

            if recv.is_err() {
                panic!("panic inserting route '{}': {:?}", route, recv);
            }
        }

        let tsr_routes = vec![
            "/hi/",
            "/b",
            "/search/gopher/",
            "/cmd/vet",
            "/src",
            "/x/",
            "/y",
            "/0/go/",
            "/1/go",
            "/a",
            "/admin/",
            "/admin/config/",
            "/admin/config/permissions/",
            "/doc/",
        ];

        for route in tsr_routes {
            let guard = match tree.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let (handler, _, tsr) = guard.get_value(route);

            if handler.is_some() {
                panic!("non-nil handler for TSR route '{}'", route);
            } else if !tsr {
                panic!("expected TSR recommendation for route '{}'", route);
            }
        }

        let no_tsr_routes = vec!["/", "/no", "/no/", "/_", "/_/", "/api/world/abc"];

        for route in no_tsr_routes {
            let guard = match tree.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let (handler, _, tsr) = guard.get_value(route);

            if handler.is_some() {
                panic!("non-nil handler for TSR route '{}'", route);
            } else if tsr {
                panic!("expected TSR recommendation for route '{}'", route);
            }
        }
    }

    #[test]
    fn test_tree_root_trailing_slash_redirect() {
        let tree = Mutex::new(Node::new());

        let recv = panic::catch_unwind(|| {
            let mut guard = match tree.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.add_route("/:test", fake_handler("/:test"));
        });

        if recv.is_err() {
            panic!("panic inserting test route: {:?}", recv);
        }

        let guard = match tree.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let (handler, _, tsr) = guard.get_value("/");

        if handler.is_some() {
            panic!("non-nil handler");
        } else if tsr {
            panic!("expected no TSR recommendation");
        }
    }

    #[test]
    fn test_tree_find_case_insensitive_path() {
        // let tree = Mutex::new(Node::new());
        let mut tree = Node::new();

        let routes = vec![
            "/hi",
            "/b/",
            "/ABC/",
            "/search/:query",
            "/cmd/:tool/",
            "/src/*filepath",
            "/x",
            "/x/y",
            "/y/",
            "/y/z",
            "/0/:id",
            "/0/:id/1",
            "/1/:id/",
            "/1/:id/2",
            "/aa",
            "/a/",
            "/doc",
            "/doc/go_faq.html",
            "/doc/go1.html",
            "/doc/go/away",
            "/no/a",
            "/no/b",
            "/Π",
            "/u/apfêl/",
            "/u/äpfêl/",
            "/u/öpfêl",
            "/v/Äpfêl/",
            "/v/Öpfêl",
            "/w/♬",   // 3 byte
            "/w/♭/",  // 3 byte, last byte differs
            "/w/𠜎",  // 4 byte
            "/w/𠜏/", // 4 byte
        ];

        for route in &routes {
            // let mut recv = panic::catch_unwind(|| {
            //     let mut guard = match tree.lock() {
            //         Ok(guard) => guard,
            //         Err(poisoned) => poisoned.into_inner(),
            //     };
            //     guard.add_route(route, fake_handler(route));
            // });

            // if recv.is_err() {
            //     panic!("panic inserting route '{}': {:?}", route, recv);
            // }
            tree.add_route(route, fake_handler(route));
        }

        for route in &routes {
            // let mut guard = match tree.lock() {
            //     Ok(guard) => guard,
            //     Err(poisoned) => poisoned.into_inner(),
            // };
            // let (out, found) = guard.find_case_insensitive_path(route, false);
            let (out, found) = tree.find_case_insensitive_path(route, false);
            // println!("{},{}", str::from_utf8(&out).unwrap(), found);
            if !found {
                panic!("Route '{}' not found!", route);
            // println!("Route '{}' not found!", route);
            } else if out != *route {
                panic!("Wrong result for route '{}': {}", route, out);
            }
        }
    }
}
