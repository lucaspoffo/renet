fn main() {
    // see https://rustwasm.github.io/docs/wasm-bindgen/web-sys/unstable-apis.html
    println!("cargo:rustc-cfg=web_sys_unstable_apis");
}
