use glib_build_tools::compile_resources;

fn main() {
    compile_resources(
        &["resources"],
        "resources/gresource.xml",
        "resources.gresource",
    );
}
