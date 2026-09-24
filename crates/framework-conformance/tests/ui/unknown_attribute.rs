use framework_core::rsx;

fn main() {
    let _ = rsx! { <Column key="root" paddng={framework_core::EdgeInsets::all(4)}></Column> };
}
