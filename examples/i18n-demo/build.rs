//! Types the example's messages from `locales/*.ftl`, and embeds its
//! resources (`rustnative.toml`).

fn main() {
    framework_build::compile_messages();
    framework_build::embed_resources();
}
