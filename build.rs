use glib_build_tools::compile_resources;

fn main() {
    compile_resources(
        &["resources"],
        "resources/gresource.xml",
        "resources.gresource",
    );

    prost_build::compile_protos(&["proto/ipc.proto"], &["proto/"]).unwrap();
}
