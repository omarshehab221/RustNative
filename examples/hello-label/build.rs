//! Embeds the example's version information and Windows application
//! manifest — per-monitor DPI awareness, Common Controls v6, and the
//! `supportedOS` entries layered child windows need — all described by
//! `rustnative.toml` — and compiles `app.css` into its theme.

fn main() {
    framework_build::compile_styles();
    framework_build::embed_resources();
}
