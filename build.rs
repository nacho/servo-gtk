use glib_build_tools::compile_resources;

fn main() {
    // Compile Servo's built-in resources (HSTS preload list, public suffix
    // list, debugger script, ...) into a GResource bundle that the runner
    // registers as Servo's resource reader.
    compile_resources(
        &["resources"],
        "resources/gresource.xml",
        "resources.gresource",
    );

    // `compile_resources` emits its own `rerun-if-changed` lines, which
    // disables Cargo's default "rerun on any change", so the proto must be
    // listed explicitly or edits to it are silently ignored.
    println!("cargo:rerun-if-changed=proto/ipc.proto");
    prost_build::compile_protos(&["proto/ipc.proto"], &["proto/"]).unwrap();
}
