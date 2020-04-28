use crate::core::router::tree::Node;
use std::collections::BTreeMap;
use std::ops::Index;
use std::path::Path;
use crate::core::router::path::clean_path;

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub key: String,
    pub value: String,
}
impl Param {
    pub fn new(key: &str, value: &str) -> Param {
        Param {
            key: key.to_string(),
            value: value.to_string(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Params(pub Vec<Param>);

impl Params {
    /// ByName returns the value of the first Param which key matches the given name.
    /// If no matching Param is found, an empty string is returned.
    pub fn by_name(&self, name: &str) -> Option<&str> {
        match self.0.iter().find(|param| param.key == name) {
            Some(param) => Some(&param.value),
            None => None,
        }
    }

    /// Empty `Params`
    pub fn new() -> Params {
        Params(Vec::new())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push(&mut self, p: Param) {
        self.0.push(p);
    }
}

impl Index<usize> for Params {
    type Output = str;

    fn index(&self, i: usize) -> &Self::Output {
        &(self.0)[i].value
    }
}


#[allow(dead_code)]
pub struct Route<T> {
    pub trees: BTreeMap<String, Node<T>>,

    // Enables automatic redirection if the current route can't be matched but a
    // handler for the path with (without) the trailing slash exists.
    // For example if /foo/ is requested but a route only exists for /foo, the
    // client is redirected to /foo with http status code 301 for GET requests
    // and 307 for all other request methods.
    pub redirect_trailing_slash: bool,

    // If enabled, the router tries to fix the current request path, if no
    // handle is registered for it.
    // First superfluous path elements like ../ or // are removed.
    // Afterwards the router does a case-insensitive lookup of the cleaned path.
    // If a handle can be found for this route, the router makes a redirection
    // to the corrected path with status code 301 for GET requests and 307 for
    // all other request methods.
    // For example /FOO and /..//Foo could be redirected to /foo.
    // RedirectTrailingSlash is independent of this option.
    pub redirect_fixed_path: bool,

    // If enabled, the router checks if another method is allowed for the
    // current route, if the current request can not be routed.
    // If this is the case, the request is answered with 'Method Not Allowed'
    // and HTTP status code 405.
    // If no other Method is allowed, the request is delegated to the NotFound
    // handler.
    pub handle_method_not_allowed: bool,

    // If enabled, the router automatically replies to OPTIONS requests.
    // Custom OPTIONS handlers take priority over automatic replies.
    pub handle_options: bool,

    // Configurable handler which is called when no matching route is
    // found.
    pub not_found: Option<T>,

    // Configurable handler which is called when a request
    // cannot be routed and HandleMethodNotAllowed is true.
    // The "Allow" header with allowed request methods is set before the handler
    // is called.
    pub method_not_allowed: Option<T>,

    // Function to handle panics recovered from http handlers.
    // It should be used to generate a error page and return the http error code
    // 500 (Internal Server Error).
    // The handler can be used to keep your server from crashing because of
    // unrecovered panics.
    pub panic_handler: Option<T>,
}

impl<T> Route<T> {
    /// New returns a new initialized Router.
    /// Path auto-correction, including trailing slashes, is enabled by default.
    pub fn new() -> Route<T> {
        Route {
            trees: BTreeMap::new(),
            redirect_trailing_slash: true,
            redirect_fixed_path: true,
            handle_method_not_allowed: true,
            handle_options: true,
            not_found: None,
            method_not_allowed: None,
            panic_handler: None,
        }
    }

    /// get is a shortcut for router.handle("GET", path, handle)
    pub fn get(&mut self, path: &str, handle: T) {
        self.handle("GET", path, handle);
    }

    /// head is a shortcut for router.handle("HEAD", path, handle)
    pub fn head(&mut self, path: &str, handle: T) {
        self.handle("HEAD", path, handle);
    }

    /// options is a shortcut for router.handle("OPTIONS", path, handle)
    pub fn options(&mut self, path: &str, handle: T) {
        self.handle("OPTIONS", path, handle);
    }

    /// post is a shortcut for router.handle("POST", path, handle)
    pub fn post(&mut self, path: &str, handle: T) {
        self.handle("POST", path, handle);
    }

    /// put is a shortcut for router.handle("PUT", path, handle)
    pub fn put(&mut self, path: &str, handle: T) {
        self.handle("PUT", path, handle);
    }

    /// patch is a shortcut for router.handle("PATCH", path, handle)
    pub fn patch(&mut self, path: &str, handle: T) {
        self.handle("PATCH", path, handle);
    }

    /// delete is a shortcut for router.handle("DELETE", path, handle)
    pub fn delete(&mut self, path: &str, handle: T) {
        self.handle("DELETE", path, handle);
    }

    /// Unimplemented. Perhaps something like
    ///
    /// # Example
    ///
    /// ```ignore
    /// router.group(vec![middelware], |router| {
    ///     router.get("/something", somewhere);
    ///     router.post("/something", somewhere);
    /// })
    /// ```
    pub fn group() {
        unimplemented!()
    }

    /// Handle registers a new request handle with the given path and method.
    ///
    /// For GET, POST, PUT, PATCH and DELETE requests the respective shortcut
    /// functions can be used.
    ///
    /// This function is intended for bulk loading and to allow the usage of less
    /// frequently used, non-standardized or custom methods (e.g. for internal
    /// communication with a proxy).
    pub fn handle(&mut self, method: &str, path: &str, handle: T) {
        if !path.starts_with("/") {
            panic!("path must begin with '/' in path '{}'", path);
        }

        self.trees
            .entry(method.to_string())
            .or_insert(Node::new())
            .add_route(path, handle);
    }

    /// Lookup allows the manual lookup of a method + path combo.
    ///
    /// This is e.g. useful to build a framework around this router.
    ///
    /// If the path was found, it returns the handle function and the path parameter
    /// values. Otherwise the third return value indicates whether a redirection to
    /// the same path with an extra / without the trailing slash should be performed.
    pub fn lookup(&mut self, method: &str, path: &str) -> (Option<&T>, Params, bool) {
        self.trees
            .get_mut(method)
            .and_then(|n| Some(n.get_value(path)))
            .unwrap_or((None, Params::new(), false))
    }

    pub fn allowed(&self, path: &str, req_method: &str) -> String {
        let mut allow = String::new();
        if path == "*" {
            for method in self.trees.keys() {
                if method == "OPTIONS" {
                    continue;
                }

                if allow.is_empty() {
                    allow.push_str(method);
                } else {
                    allow.push_str(", ");
                    allow.push_str(method);
                }
            }
        } else {
            for method in self.trees.keys() {
                if method == req_method || method == "OPTIONS" {
                    continue;
                }

                self.trees.get(method).map(|tree| {
                    let (handle, _, _) = tree.get_value(path);

                    if handle.is_some() {
                        if allow.is_empty() {
                            allow.push_str(method);
                        } else {
                            allow.push_str(", ");
                            allow.push_str(method);
                        }
                    }
                });
            }
        }

        if allow.len() > 0 {
            allow += ", OPTIONS";
        }

        allow
    }
}