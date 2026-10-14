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

    prost_build::compile_protos(&["proto/ipc.proto"], &["proto/"]).unwrap();
}
