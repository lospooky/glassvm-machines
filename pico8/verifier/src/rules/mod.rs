//! Static PICO-8 compatibility rules.

const UNSUPPORTED_APIS: &[&str] = &[
    "all", "cartdata", "circ", "circfill", "cocreate", "coresume", "costatus", "cstore", "cursor",
    "del", "dget", "dset", "extcmd", "fetch", "fillp", "foreach", "load", "map", "menuitem",
    "oval", "ovalfill", "reload", "serial", "split", "tline", "yield",
];

pub fn unsupported_api_calls(source: &str) -> Vec<String> {
    UNSUPPORTED_APIS
        .iter()
        .filter(|name| source.contains(&format!("{name}(")))
        .map(|name| (*name).to_owned())
        .collect()
}
