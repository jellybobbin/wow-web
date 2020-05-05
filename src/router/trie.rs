
struct TrieNode {
    children: Vec<TrieNode>,
    has_dynamic_child: bool,
    child_named_parameter: bool,
    child_wildcard_parameter: bool,
    param_keys: Vec<String>,
    end: bool,
    key: String,
    static_key: String,
    //handlers : context.Handlers,
    route_name: String
}