//! Compiles the Slint markup with the Fluent style (std-widgets' `ListView` and scroll bars). The
//! style's SVG icons are rasterized at build time for the software renderer, so the exe needs no
//! SVG or image decoder at run time. Also embeds the shared application manifest.

include!("../manifest.rs");

fn main() {
    embed_manifest();
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer);
    if let Err(e) = slint_build::compile_with_config("ui/app.slint", config) {
        // A build script reports failure by exiting non-zero; the message goes to cargo's output.
        eprintln!("slint: {e}");
        std::process::exit(1);
    }
}
